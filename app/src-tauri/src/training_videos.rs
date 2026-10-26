//! Training videos (#190) download the first time they are needed, so a
//! changed training video needs no release and the installer carries none.
//!
//! The website publishes them at [`SOURCE`] with a manifest, `videos.json`
//! (`lampstand_library::training::VideoManifest`, written by
//! `node training/build.mjs publish`): each video's file, size and BLAKE3
//! hash. The hash is the video's blob hash, so a training service names
//! its videos before they arrive, and the renderer plays one from the blob
//! store like any other once it has.
//!
//! Opening a training service (or Reset training) calls
//! [`TrainingVideos::ensure`], which starts a download on its own thread
//! when something is missing or the manifest was last checked more than a
//! day ago. Nothing else downloads, and nothing at start. Each video is
//! streamed to a temporary file in `training-videos/`, refused unless its
//! size and hash match the manifest, and put in the blob store, which
//! renames it into place. The manifest last fetched is kept in
//! `training-videos/videos.json` with when it was fetched, so videos
//! already here keep playing offline. Until a video arrives its item shows
//! [`OFFLINE`] (or [`DOWNLOADING`]) instead, and the rest of the training
//! service runs as before.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use lampstand_api::{Event, TrainingVideoState, TrainingVideos as View};
use lampstand_core::id::BlobHash;
use lampstand_library::training::{TrainingVideo, VideoManifest};
use lampstand_library::Library;
use serde::{Deserialize, Serialize};

use crate::handler::EventSink;

/// Where the website serves the manifest and the videos.
pub const SOURCE: &str = "https://lampstand.dbain.com/training/";

/// The manifest as published when this build was made: the videos the
/// training services name before the website has been reached once.
pub const PUBLISHED: &str = include_str!("../../../site/training/videos.json");

/// What a video item shows, and Present says, while a training video is
/// not on this device and could not be downloaded.
pub const OFFLINE: &str =
    "The training videos download the first time. Connect to the internet to watch them.";

/// What a video item shows while the training videos are downloading.
pub const DOWNLOADING: &str =
    "The training videos are downloading. This one plays here when it has arrived.";

/// Before the first try.
pub const NOT_DOWNLOADED: &str =
    "The training videos download the first time you open a training service.";

/// What a video item shows when the website's list, fetched, does not name
/// its video yet: a release may ship an item before its video is published.
pub const NOT_READY: &str = "This training video isn't ready yet. Check back soon.";

/// The blob hash a video item names when no manifest lists its video yet.
/// No file has it, so the item shows a message until a list names the
/// video, and then it is pointed at the real one.
pub fn unpublished(file: &str) -> BlobHash {
    BlobHash::of(format!("lampstand: training video {file} is not published yet").as_bytes())
}

/// How often the manifest is fetched again, in milliseconds: once a day.
const RECHECK_MS: i64 = 24 * 60 * 60 * 1000;

/// After a download that failed (offline, a missing file, a damaged one),
/// opening a training service does not try again for this long. Reset
/// training always does.
const BACKOFF: Duration = Duration::from_secs(10 * 60);

/// The largest manifest read: it is a short list.
pub const MAX_MANIFEST_BYTES: u64 = 64 * 1024;

/// The largest training video downloaded, as for a sound file.
pub const MAX_VIDEO_BYTES: u64 = 250 * 1024 * 1024;

/// The most all the training videos together may take.
pub const MAX_TOTAL_BYTES: u64 = 1024 * 1024 * 1024;

/// The manifest last fetched, in `training-videos/`.
const SAVED: &str = "videos.json";

#[derive(Debug, Default, Serialize, Deserialize)]
struct Saved {
    /// When the website last answered, in ms since 1970.
    checked_at: Option<i64>,
    manifest: Option<VideoManifest>,
    /// The videos the training services play: a changed video replaces
    /// its old one only once it is here, so this can lag `manifest`.
    #[serde(default)]
    installed: Option<VideoManifest>,
}

/// What a download that ended changed, from [`TrainingVideos::take_changes`].
#[derive(Debug, Default, PartialEq)]
pub struct Changes {
    /// Each video whose hash moved, old to new, now that the new one is
    /// here: point the training services at it.
    pub moved: Vec<(BlobHash, BlobHash)>,
    /// A video arrived, so items waiting for it can play.
    pub arrived: bool,
}

impl Changes {
    /// Whether a training service shows anything new.
    pub fn any(&self) -> bool {
        self.arrived || !self.moved.is_empty()
    }
}

/// The training videos on this device, and their download.
pub struct TrainingVideos {
    dir: PathBuf,
    /// `None`: never download (tests that must stay offline).
    source: Option<String>,
    library: Arc<Library>,
    emit: EventSink,
    shared: Arc<(Mutex<Shared>, Condvar)>,
}

struct Shared {
    /// The manifest the training services were built or last updated from.
    installed: VideoManifest,
    /// The newest manifest known, which the downloads follow: fetched,
    /// else saved, else published.
    latest: VideoManifest,
    checked_at: Option<i64>,
    running: bool,
    /// The end of the last download has been told (`Event::TrainingVideos`),
    /// so `wait` returns only after the final view has gone out.
    told: bool,
    /// A download was tried since the app started.
    tried: bool,
    /// Bytes done and to do, while downloading.
    progress: Option<(u64, u64)>,
    /// A download ended since `take_changes`.
    ended: bool,
    /// A video arrived since `take_changes`.
    arrived: bool,
    /// When the last download failed, for [`BACKOFF`].
    failed_at: Option<Instant>,
    /// Reset training asked for a new look while a download ran.
    recheck_pending: bool,
}

impl TrainingVideos {
    /// Reads what an earlier run saved in `dir`. Never touches the network.
    pub fn open(
        dir: PathBuf,
        source: Option<String>,
        library: Arc<Library>,
        emit: EventSink,
    ) -> Self {
        // Temporary files a crash mid-download left.
        for entry in fs::read_dir(&dir).into_iter().flatten().flatten() {
            if entry.file_name().to_string_lossy().starts_with(".tmp-") {
                let _ = fs::remove_file(entry.path());
            }
        }
        let saved: Saved = fs::read(dir.join(SAVED))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let published = published();
        let installed = installed_at_start(&saved, &published, &library);
        let latest = saved.manifest.unwrap_or(published);
        Self {
            dir,
            source: source.map(|s| if s.ends_with('/') { s } else { s + "/" }),
            library,
            emit,
            shared: Arc::new((
                Mutex::new(Shared {
                    installed,
                    latest,
                    checked_at: saved.checked_at,
                    running: false,
                    told: true,
                    tried: false,
                    progress: None,
                    ended: false,
                    arrived: false,
                    failed_at: None,
                    recheck_pending: false,
                }),
                Condvar::new(),
            )),
        }
    }

    /// Where the manifest and videos come from; `None` never downloads.
    pub fn set_source(&mut self, source: Option<String>) {
        self.source = source.map(|s| if s.ends_with('/') { s } else { s + "/" });
    }

    fn lock(&self) -> MutexGuard<'_, Shared> {
        self.shared.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The manifest the training services name their videos by.
    pub fn manifest(&self) -> VideoManifest {
        self.lock().installed.clone()
    }

    /// Where the videos stand, for Present.
    pub fn view(&self) -> View {
        view(&self.lock(), &self.library)
    }

    /// What a video item that is not here yet shows instead.
    /// `blob` is the video item's: a video the website's list does not
    /// name yet, once the list has been fetched, is [`NOT_READY`], not a
    /// problem with the connection.
    pub fn missing_message(&self, blob: &BlobHash) -> &'static str {
        let shared = self.lock();
        let listed = shared
            .latest
            .videos
            .iter()
            .any(|v| v.blake3 == *blob || unpublished(&v.file) == *blob);
        if !listed && shared.checked_at.is_some() {
            NOT_READY
        } else if shared.running {
            DOWNLOADING
        } else {
            OFFLINE
        }
    }

    /// Starts a download if a training video is missing, or the manifest
    /// is a day old, or `recheck` (Reset training) asks for a new look.
    /// Without `recheck`, nothing is tried within [`BACKOFF`] of a failed
    /// download. Returns at once; the download runs on its own thread.
    pub fn ensure(&self, recheck: bool) {
        let Some(source) = self.source.clone() else {
            return;
        };
        {
            let mut shared = self.lock();
            if shared.running {
                // Looked at again once this download ends.
                shared.recheck_pending |= recheck;
                return;
            }
            if !recheck && shared.failed_at.is_some_and(|t| t.elapsed() < BACKOFF) {
                return;
            }
            let stale = recheck || is_stale(shared.checked_at, now_ms());
            if !stale && missing(&shared.latest, &self.library).is_empty() {
                return;
            }
            shared.running = true;
            shared.told = false;
            shared.tried = true;
        }
        (self.emit)(Event::TrainingVideos(self.view()));
        let job = Job {
            dir: self.dir.clone(),
            source,
            library: self.library.clone(),
            emit: self.emit.clone(),
            shared: self.shared.clone(),
            recheck,
        };
        let started = std::thread::Builder::new()
            .name("training-videos".into())
            .spawn(move || job.run());
        if let Err(e) = started {
            eprintln!("lampstand: the training videos cannot download: {e}");
            let mut shared = self.lock();
            shared.running = false;
            shared.told = true;
            self.shared.1.notify_all();
        }
    }

    /// `Some` once after each download ends, however it went, saying what
    /// changed. A changed video replaces its old one only once it is on
    /// this device; until then the old one keeps playing, and the switch
    /// waits for a later download.
    pub fn take_changes(&self) -> Option<Changes> {
        let mut shared = self.lock();
        if !shared.ended {
            return None;
        }
        shared.ended = false;
        let here = |hash: &BlobHash| self.library.repo().blobs().path(hash).is_some();
        let mut changes = Changes {
            moved: Vec::new(),
            arrived: std::mem::take(&mut shared.arrived),
        };
        let mut installed = shared.installed.clone();
        for new in &shared.latest.videos {
            if !here(&new.blake3) {
                continue;
            }
            match installed.videos.iter_mut().find(|v| v.file == new.file) {
                Some(old) if old.blake3 != new.blake3 => {
                    changes.moved.push((old.blake3, new.blake3));
                    *old = new.clone();
                }
                Some(_) => {}
                // A video published after the release that names it.
                None => {
                    changes.moved.push((unpublished(&new.file), new.blake3));
                    installed.videos.push(new.clone());
                }
            }
        }
        if installed != shared.installed {
            shared.installed = installed;
            let saved = Saved {
                checked_at: shared.checked_at,
                manifest: Some(shared.latest.clone()),
                installed: Some(shared.installed.clone()),
            };
            if let Err(e) = save(&self.dir, &saved) {
                eprintln!("lampstand: the training video list was not saved: {e}");
            }
        }
        Some(changes)
    }

    /// Waits for a running download to finish (tests).
    pub fn wait(&self) {
        let (lock, done) = &*self.shared;
        let mut shared = lock.lock().unwrap_or_else(|e| e.into_inner());
        while shared.running || !shared.told {
            shared = done.wait(shared).unwrap_or_else(|e| e.into_inner());
        }
    }
}

/// The manifest this build was made with.
pub fn published() -> VideoManifest {
    serde_json::from_str(PUBLISHED).expect("site/training/videos.json is a manifest")
}

/// The videos the training services play at start, file by file: the one
/// they played last time, else the website's latest, else the one this
/// build was published with, whichever is first on this device; failing
/// that, the latest known, so it downloads. A video a newer release names
/// before its website list was fetched comes from the published list.
fn installed_at_start(
    saved: &Saved,
    published: &VideoManifest,
    library: &Library,
) -> VideoManifest {
    let here = |v: &&TrainingVideo| library.repo().blobs().path(&v.blake3).is_some();
    let lists: Vec<&VideoManifest> = [saved.installed.as_ref(), saved.manifest.as_ref()]
        .into_iter()
        .flatten()
        .chain([published])
        .collect();
    let mut out = VideoManifest {
        version: VideoManifest::VERSION,
        videos: Vec::new(),
    };
    for list in &lists {
        for video in &list.videos {
            if out.video(&video.file).is_some() {
                continue;
            }
            let candidates = || lists.iter().filter_map(|l| l.video(&video.file));
            let chosen = candidates()
                .find(here)
                .or_else(|| saved.manifest.as_ref().and_then(|m| m.video(&video.file)))
                .or_else(|| candidates().next())
                .expect("listed above");
            out.videos.push(chosen.clone());
        }
    }
    out
}

fn is_stale(checked_at: Option<i64>, now: i64) -> bool {
    checked_at.is_none_or(|t| now - t >= RECHECK_MS || now < t)
}

fn missing(manifest: &VideoManifest, library: &Library) -> Vec<TrainingVideo> {
    manifest
        .videos
        .iter()
        .filter(|v| library.repo().blobs().path(&v.blake3).is_none())
        .cloned()
        .collect()
}

fn view(shared: &Shared, library: &Library) -> View {
    let state = if let Some((done, total)) = shared.progress {
        let kb = |n: u64| u32::try_from(n).unwrap_or(u32::MAX);
        TrainingVideoState::Downloading {
            done: kb(done),
            total: kb(total),
        }
    } else if missing(&shared.latest, library).is_empty() {
        TrainingVideoState::Ready
    } else if shared.running {
        TrainingVideoState::Downloading { done: 0, total: 0 }
    } else if shared.tried {
        TrainingVideoState::Offline
    } else {
        TrainingVideoState::NotDownloaded
    };
    let message = match state {
        TrainingVideoState::Ready => "",
        TrainingVideoState::NotDownloaded => NOT_DOWNLOADED,
        TrainingVideoState::Downloading { .. } => DOWNLOADING,
        TrainingVideoState::Offline => OFFLINE,
    };
    View {
        state,
        message: message.to_string(),
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// One download, on its own thread.
struct Job {
    dir: PathBuf,
    source: String,
    library: Arc<Library>,
    emit: EventSink,
    shared: Arc<(Mutex<Shared>, Condvar)>,
    recheck: bool,
}

impl Job {
    fn lock(&self) -> MutexGuard<'_, Shared> {
        self.shared.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn tell(&self) {
        let view = view(&self.lock(), &self.library);
        (self.emit)(Event::TrainingVideos(view));
    }

    fn run(mut self) {
        let failed = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => loop {
                let ok = runtime.block_on(self.download());
                // Reset training while this ran: look again now.
                let again = std::mem::take(&mut self.lock().recheck_pending);
                if !again {
                    break !ok;
                }
                self.recheck = true;
            },
            Err(e) => {
                eprintln!("lampstand: the training videos cannot download: {e}");
                true
            }
        };
        {
            let mut shared = self.lock();
            shared.running = false;
            shared.progress = None;
            shared.ended = true;
            shared.failed_at = failed.then(Instant::now);
            self.shared.1.notify_all();
        }
        // Told outside the lock; then whoever waits may read it.
        self.tell();
        self.lock().told = true;
        self.shared.1.notify_all();
    }

    /// One look at the website and the downloads it calls for. False if
    /// anything failed: the list could not be fetched when it was due, or
    /// a video did not arrive.
    async fn download(&self) -> bool {
        let client = match reqwest::Client::builder()
            .user_agent(concat!("lampstand/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(30))
            // Never from https to plain http by a redirect.
            .https_only(self.source.starts_with("https://"))
            .build()
        {
            Ok(client) => client,
            Err(e) => {
                eprintln!("lampstand: the training videos cannot download: {e}");
                return false;
            }
        };
        let mut ok = true;
        let now = now_ms();
        let stale = self.recheck || is_stale(self.lock().checked_at, now);
        if stale {
            match fetch_manifest(&client, &self.source).await {
                Ok(fetched) => {
                    // The website's list is the one to follow.
                    let mut shared = self.lock();
                    shared.checked_at = Some(now);
                    shared.latest = fetched;
                    let saved = Saved {
                        checked_at: Some(now),
                        manifest: Some(shared.latest.clone()),
                        installed: Some(shared.installed.clone()),
                    };
                    if let Err(e) = save(&self.dir, &saved) {
                        eprintln!("lampstand: the training video list was not saved: {e}");
                    }
                }
                Err(e) => {
                    eprintln!("lampstand: the training video list is unavailable: {e}");
                    ok = false;
                }
            }
        }

        let wanted = missing(&self.lock().latest, &self.library);
        if wanted.is_empty() {
            return ok;
        }
        let total = wanted.iter().map(|v| v.size).sum();
        self.lock().progress = Some((0, total));
        self.tell();
        let mut done = 0;
        let mut told = Instant::now();
        for video in &wanted {
            let before = done;
            let result = fetch_video(
                &client,
                &self.source,
                video,
                &self.dir,
                &self.library,
                |n| {
                    done += n;
                    self.lock().progress = Some((done, total));
                    if told.elapsed() >= Duration::from_millis(250) {
                        told = Instant::now();
                        self.tell();
                    }
                },
            )
            .await;
            done = before + video.size;
            match result {
                Ok(()) => self.lock().arrived = true,
                Err(e) => {
                    eprintln!("lampstand: training video {}: {e}", video.file);
                    ok = false;
                }
            }
            self.lock().progress = Some((done, total));
        }
        ok
    }
}

fn save(dir: &Path, saved: &Saved) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".tmp-{SAVED}"));
    fs::write(&tmp, serde_json::to_vec_pretty(saved)?)?;
    fs::rename(&tmp, dir.join(SAVED))
}

async fn fetch_manifest(client: &reqwest::Client, source: &str) -> Result<VideoManifest, String> {
    let mut response = client
        .get(format!("{source}videos.json"))
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.to_string())?;
    let too_big = || format!("the list is larger than {MAX_MANIFEST_BYTES} bytes");
    if response
        .content_length()
        .is_some_and(|n| n > MAX_MANIFEST_BYTES)
    {
        return Err(too_big());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        if (bytes.len() + chunk.len()) as u64 > MAX_MANIFEST_BYTES {
            return Err(too_big());
        }
        bytes.extend_from_slice(&chunk);
    }
    let manifest: VideoManifest = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    check_manifest(&manifest)?;
    Ok(manifest)
}

/// Refuses a list that names anything but plain video files, or more than
/// this device should download: it comes from the network.
fn check_manifest(manifest: &VideoManifest) -> Result<(), String> {
    if let Some(bad) = manifest.videos.iter().find(|v| !safe_name(&v.file)) {
        return Err(format!(
            "the list names {:?}, which is not a video file",
            bad.file
        ));
    }
    if let Some(big) = manifest.videos.iter().find(|v| v.size > MAX_VIDEO_BYTES) {
        return Err(format!(
            "{} is {} bytes, more than a training video may be",
            big.file, big.size
        ));
    }
    let total: u64 = manifest.videos.iter().map(|v| v.size).sum();
    if total > MAX_TOTAL_BYTES {
        return Err(format!(
            "the videos come to {total} bytes, more than the training videos may"
        ));
    }
    Ok(())
}

/// A plain file name ending in `.mp4`: the manifest comes from the network.
fn safe_name(file: &str) -> bool {
    file.ends_with(".mp4")
        && !file.starts_with('.')
        && file
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// Streams one video to a temporary file in `dir`, refuses it unless it is
/// the size and hash the manifest says, and puts it in the blob store. The
/// temporary file is gone afterwards either way.
async fn fetch_video(
    client: &reqwest::Client,
    source: &str,
    video: &TrainingVideo,
    dir: &Path,
    library: &Library,
    mut progress: impl FnMut(u64),
) -> Result<(), String> {
    if !safe_name(&video.file) {
        return Err("not a video file".into());
    }
    if video.size > MAX_VIDEO_BYTES {
        return Err("larger than a training video may be".into());
    }
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!(".tmp-{}", video.file));
    let result = async {
        let mut response = client
            .get(format!("{source}{}", video.file))
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| e.to_string())?;
        if response.content_length().is_some_and(|n| n != video.size) {
            return Err("the website has a different size of it".to_string());
        }
        let mut file = File::create(&tmp).map_err(|e| e.to_string())?;
        let mut hasher = blake3::Hasher::new();
        let mut written = 0u64;
        while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
            written += chunk.len() as u64;
            // `size` is at most MAX_VIDEO_BYTES, so this caps the stream too.
            if written > video.size || written > MAX_VIDEO_BYTES {
                return Err("it is larger than the list says".into());
            }
            hasher.update(&chunk);
            file.write_all(&chunk).map_err(|e| e.to_string())?;
            progress(chunk.len() as u64);
        }
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        if written != video.size {
            return Err(format!("{written} of {} bytes arrived", video.size));
        }
        let hash = BlobHash::from_bytes(*hasher.finalize().as_bytes());
        if hash != video.blake3 {
            return Err(format!("damaged on the way (hash {hash})"));
        }
        let file = File::open(&tmp).map_err(|e| e.to_string())?;
        library
            .repo()
            .blobs()
            .put_verified(&video.blake3, file)
            .map_err(|e| e.to_string())
    }
    .await;
    let _ = fs::remove_file(&tmp);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use lampstand_sync::team::DeviceKeypair;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct Setup {
        _dir: tempfile::TempDir,
        data: PathBuf,
        library: Arc<Library>,
        events: Arc<Mutex<Vec<Event>>>,
    }

    fn setup() -> Setup {
        let dir = tempfile::tempdir().unwrap();
        let library = Arc::new(
            Library::open(&dir.path().join("library"), DeviceKeypair::generate()).unwrap(),
        );
        Setup {
            data: dir.path().join("training-videos"),
            _dir: dir,
            library,
            events: Arc::default(),
        }
    }

    impl Setup {
        fn videos(&self, source: Option<String>) -> TrainingVideos {
            let events = self.events.clone();
            TrainingVideos::open(
                self.data.clone(),
                source,
                self.library.clone(),
                Arc::new(move |e| events.lock().unwrap().push(e)),
            )
        }
        fn has(&self, bytes: &[u8]) -> bool {
            self.library
                .repo()
                .blobs()
                .path(&BlobHash::of(bytes))
                .is_some()
        }
        /// Nothing but the saved list is left in the download folder.
        fn no_temp_files(&self) {
            for entry in fs::read_dir(&self.data).into_iter().flatten() {
                let name = entry.unwrap().file_name().to_string_lossy().into_owned();
                assert_eq!(name, SAVED, "{name} was left behind");
            }
        }
    }

    fn manifest(videos: &[(&str, &[u8])]) -> VideoManifest {
        VideoManifest {
            version: 1,
            videos: videos
                .iter()
                .map(|(file, bytes)| TrainingVideo {
                    id: file.trim_end_matches(".mp4").into(),
                    file: (*file).into(),
                    size: bytes.len() as u64,
                    blake3: BlobHash::of(bytes),
                })
                .collect(),
        }
    }

    async fn serve(server: &MockServer, list: &VideoManifest, files: &[(&str, &[u8])]) {
        server.reset().await;
        Mock::given(method("GET"))
            .and(path("/training/videos.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(list))
            .mount(server)
            .await;
        for (file, bytes) in files {
            Mock::given(method("GET"))
                .and(path(format!("/training/{file}")))
                .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes.to_vec()))
                .mount(server)
                .await;
        }
    }

    const WELCOME: &[u8] = b"a welcome video";
    const LIVE: &[u8] = b"a going-live video";

    #[tokio::test]
    async fn the_first_open_downloads_every_video() {
        let server = MockServer::start().await;
        let files: [(&str, &[u8]); 2] = [("t1-welcome.mp4", WELCOME), ("t1-going-live.mp4", LIVE)];
        serve(&server, &manifest(&files), &files).await;
        let s = setup();
        let videos = s.videos(Some(format!("{}/training", server.uri())));
        assert_eq!(videos.view().state, TrainingVideoState::NotDownloaded);

        videos.ensure(false);
        videos.wait();
        assert!(s.has(WELCOME) && s.has(LIVE));
        assert_eq!(videos.view().state, TrainingVideoState::Ready);
        assert_eq!(videos.view().message, "");
        s.no_temp_files();
        // The training services hear that a video arrived.
        assert!(videos.take_changes().is_some());
        assert!(videos.take_changes().is_none());
        // Progress was told as it went, and the end.
        let told = s.events.lock().unwrap().clone();
        assert!(told.iter().any(|e| matches!(
            e,
            Event::TrainingVideos(View {
                state: TrainingVideoState::Downloading { .. },
                ..
            })
        )));
        assert!(matches!(
            told.last(),
            Some(Event::TrainingVideos(View {
                state: TrainingVideoState::Ready,
                ..
            }))
        ));
    }

    #[tokio::test]
    async fn a_video_that_does_not_match_its_hash_is_refused_and_its_temp_file_removed() {
        let server = MockServer::start().await;
        let list = manifest(&[("t1-welcome.mp4", WELCOME)]);
        // Same size, different bytes.
        let damaged = b"a welcome vide0";
        serve(&server, &list, &[("t1-welcome.mp4", damaged)]).await;
        let s = setup();
        let videos = s.videos(Some(format!("{}/training/", server.uri())));
        videos.ensure(false);
        videos.wait();
        assert!(!s.has(WELCOME) && !s.has(damaged));
        s.no_temp_files();
        let view = videos.view();
        assert_eq!(view.state, TrainingVideoState::Offline);
        assert_eq!(view.message, OFFLINE);
        // Listed, so a problem on the way, not a video still to come.
        assert_eq!(videos.missing_message(&BlobHash::of(WELCOME)), OFFLINE);
    }

    /// A website nobody answers at: a port that was free a moment ago.
    fn unreachable() -> String {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        format!("http://127.0.0.1:{port}/training/")
    }

    #[tokio::test]
    async fn a_video_already_here_plays_offline() {
        let server = MockServer::start().await;
        let files: [(&str, &[u8]); 1] = [("t1-welcome.mp4", WELCOME)];
        serve(&server, &manifest(&files), &files).await;
        let s = setup();
        {
            let first = s.videos(Some(format!("{}/training/", server.uri())));
            first.ensure(false);
            first.wait();
        }
        assert!(s.has(WELCOME));

        // A later start, offline: the saved list names the video, it is
        // here, and nothing needs the network.
        let again = s.videos(Some(unreachable()));
        assert_eq!(
            again.manifest().video("t1-welcome.mp4").unwrap().blake3,
            BlobHash::of(WELCOME)
        );
        assert_eq!(again.view().state, TrainingVideoState::Ready);
        again.ensure(true);
        again.wait();
        assert!(s.has(WELCOME));
        assert_eq!(again.view().state, TrainingVideoState::Ready);
        assert_eq!(again.take_changes(), Some(Changes::default()));
    }

    #[test]
    fn offline_with_nothing_here_gives_the_message() {
        let s = setup();
        let videos = s.videos(Some(unreachable()));
        videos.ensure(false);
        videos.wait();
        assert_eq!(videos.view().state, TrainingVideoState::Offline);
        assert_eq!(videos.view().message, OFFLINE);
        // The published list still names every video, so the training
        // services can.
        assert_eq!(videos.manifest(), published());
    }

    #[tokio::test]
    async fn a_changed_manifest_fetches_the_new_video_once_a_day_or_on_reset() {
        let server = MockServer::start().await;
        let source = format!("{}/training/", server.uri());
        let first: [(&str, &[u8]); 1] = [("t1-welcome.mp4", WELCOME)];
        serve(&server, &manifest(&first), &first).await;
        let s = setup();
        let videos = s.videos(Some(source.clone()));
        videos.ensure(false);
        videos.wait();
        videos.take_changes();

        const NEW: &[u8] = b"a better welcome video";
        let second: [(&str, &[u8]); 1] = [("t1-welcome.mp4", NEW)];
        serve(&server, &manifest(&second), &second).await;
        // Checked less than a day ago, and nothing is missing: no request.
        videos.ensure(false);
        videos.wait();
        assert!(server.received_requests().await.unwrap().is_empty());
        assert!(!s.has(NEW));

        // Reset training looks again.
        videos.ensure(true);
        videos.wait();
        assert!(s.has(NEW));
        assert_eq!(
            videos.take_changes(),
            Some(Changes {
                moved: vec![(BlobHash::of(WELCOME), BlobHash::of(NEW))],
                arrived: true
            })
        );
        assert_eq!(
            videos.manifest().video("t1-welcome.mp4").unwrap().blake3,
            BlobHash::of(NEW)
        );

        // So does a start more than a day after the last check.
        const NEWER: &[u8] = b"the best welcome video";
        let third: [(&str, &[u8]); 1] = [("t1-welcome.mp4", NEWER)];
        serve(&server, &manifest(&third), &third).await;
        let mut saved: Saved =
            serde_json::from_slice(&fs::read(s.data.join(SAVED)).unwrap()).unwrap();
        saved.checked_at = Some(now_ms() - RECHECK_MS - 1);
        save(&s.data, &saved).unwrap();
        let later = s.videos(Some(source));
        later.ensure(false);
        later.wait();
        assert!(s.has(NEWER));
        assert_eq!(
            later.take_changes(),
            Some(Changes {
                moved: vec![(BlobHash::of(NEW), BlobHash::of(NEWER))],
                arrived: true
            })
        );
    }

    #[tokio::test]
    async fn a_video_no_list_named_is_not_ready_until_one_does() {
        // A release names t9-later.mp4; neither the list it was built with
        // nor the website has it yet.
        let server = MockServer::start().await;
        let source = format!("{}/training/", server.uri());
        let first: [(&str, &[u8]); 1] = [("t1-welcome.mp4", WELCOME)];
        serve(&server, &manifest(&first), &first).await;
        let s = setup();
        let videos = s.videos(Some(source));
        let later = unpublished("t9-later.mp4");
        // Before the website answered: it may just be offline.
        assert_eq!(videos.missing_message(&later), OFFLINE);
        videos.ensure(false);
        videos.wait();
        videos.take_changes();
        assert_eq!(videos.missing_message(&later), NOT_READY);
        assert_eq!(videos.view().state, TrainingVideoState::Ready);

        const LATER: &[u8] = b"a video published later";
        let both: [(&str, &[u8]); 2] = [("t1-welcome.mp4", WELCOME), ("t9-later.mp4", LATER)];
        serve(&server, &manifest(&both), &both).await;
        videos.ensure(true);
        videos.wait();
        assert!(s.has(LATER));
        let moved = videos.take_changes().unwrap().moved;
        assert!(moved.contains(&(later, BlobHash::of(LATER))), "{moved:?}");
    }

    #[tokio::test]
    async fn a_failed_download_is_not_tried_again_for_a_while_unless_reset() {
        let server = MockServer::start().await;
        let list = manifest(&[("t1-welcome.mp4", WELCOME)]);
        // Listed, but the file is not there (a 404).
        serve(&server, &list, &[]).await;
        let s = setup();
        let videos = s.videos(Some(format!("{}/training/", server.uri())));
        videos.ensure(false);
        videos.wait();
        assert_eq!(videos.take_changes(), Some(Changes::default()));
        let before = server.received_requests().await.unwrap().len();
        for _ in 0..5 {
            videos.ensure(false);
            videos.wait();
        }
        let after = server.received_requests().await.unwrap().len();
        assert_eq!(after, before, "nothing asked again");
        assert!(videos.take_changes().is_none());
        // Reset training always tries.
        videos.ensure(true);
        videos.wait();
        assert!(server.received_requests().await.unwrap().len() > before);
    }

    #[tokio::test]
    async fn a_changed_video_replaces_the_old_one_only_once_it_is_here() {
        const NEW: &[u8] = b"a better welcome video";
        let server = MockServer::start().await;
        let source = format!("{}/training/", server.uri());
        let first: [(&str, &[u8]); 1] = [("t1-welcome.mp4", WELCOME)];
        serve(&server, &manifest(&first), &first).await;
        let s = setup();
        let videos = s.videos(Some(source.clone()));
        videos.ensure(false);
        videos.wait();
        videos.take_changes();
        let plays = |v: &TrainingVideos| v.manifest().video("t1-welcome.mp4").unwrap().blake3;
        assert_eq!(plays(&videos), BlobHash::of(WELCOME));

        // The list says A′, but A′ arrives damaged.
        let list = manifest(&[("t1-welcome.mp4", NEW)]);
        serve(
            &server,
            &list,
            &[("t1-welcome.mp4", b"a better welcome vide0")],
        )
        .await;
        videos.ensure(true);
        videos.wait();
        assert!(!s.has(NEW));
        let changes = videos.take_changes().unwrap();
        assert!(!changes.any(), "{changes:?}");
        assert_eq!(
            plays(&videos),
            BlobHash::of(WELCOME),
            "the old one still plays"
        );

        // And after a restart, offline.
        let again = s.videos(Some(unreachable()));
        assert_eq!(plays(&again), BlobHash::of(WELCOME));

        // Once A′ arrives, it replaces A.
        serve(&server, &list, &[("t1-welcome.mp4", NEW)]).await;
        let later = s.videos(Some(source));
        later.ensure(true);
        later.wait();
        assert_eq!(
            later.take_changes().unwrap().moved,
            [(BlobHash::of(WELCOME), BlobHash::of(NEW))]
        );
        assert_eq!(plays(&later), BlobHash::of(NEW));
        assert_eq!(plays(&s.videos(None)), BlobHash::of(NEW));
    }

    #[tokio::test]
    async fn a_list_too_large_or_naming_too_much_is_refused() {
        let s = &setup();
        let refused = move |list: serde_json::Value| async move {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/training/videos.json"))
                .respond_with(ResponseTemplate::new(200).set_body_json(list))
                .mount(&server)
                .await;
            let videos = s.videos(Some(format!("{}/training/", server.uri())));
            videos.ensure(true);
            videos.wait();
            let asked: Vec<String> = server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .map(|r| r.url.path().to_string())
                .collect();
            // The refused list's videos are never asked for; the list the
            // app already had stands.
            assert_eq!(asked[0], "/training/videos.json");
            assert!(
                asked.iter().all(|p| p == "/training/videos.json"
                    || published().videos.iter().any(|v| p.ends_with(&v.file))),
                "{asked:?}"
            );
            assert_eq!(videos.manifest(), published());
        };
        let entry = |file: &str, size: u64| {
            serde_json::json!({"id": file.trim_end_matches(".mp4"), "file": file, "size": size,
                "blake3": BlobHash::of(file.as_bytes()).to_string()})
        };
        // Larger than 64 KiB.
        let padding = "x".repeat(MAX_MANIFEST_BYTES as usize);
        refused(serde_json::json!({"version": 1, "videos": [], "padding": padding})).await;
        // One video over 250 MB.
        refused(
            serde_json::json!({"version": 1, "videos": [entry("big.mp4", MAX_VIDEO_BYTES + 1)]}),
        )
        .await;
        // Five under it, over 1 GB together.
        let videos: Vec<_> = (0..5)
            .map(|n| entry(&format!("v{n}.mp4"), MAX_VIDEO_BYTES))
            .collect();
        refused(serde_json::json!({"version": 1, "videos": videos})).await;
    }

    #[tokio::test]
    async fn a_video_longer_than_its_list_says_is_cut_off() {
        let server = MockServer::start().await;
        let list = manifest(&[("t1-welcome.mp4", WELCOME)]);
        Mock::given(method("GET"))
            .and(path("/training/videos.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&list))
            .mount(&server)
            .await;
        // More than the list says.
        let body = [WELCOME, b" and then a great deal more"].concat();
        Mock::given(method("GET"))
            .and(path("/training/t1-welcome.mp4"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body))
            .mount(&server)
            .await;
        let s = setup();
        let videos = s.videos(Some(format!("{}/training/", server.uri())));
        videos.ensure(false);
        videos.wait();
        assert!(!s.has(WELCOME));
        s.no_temp_files();
    }

    #[test]
    fn leftover_temp_files_are_cleared_at_start() {
        let s = setup();
        fs::create_dir_all(&s.data).unwrap();
        fs::write(s.data.join(".tmp-t1-welcome.mp4"), b"half a video").unwrap();
        let _videos = s.videos(None);
        assert!(!s.data.join(".tmp-t1-welcome.mp4").exists());
    }

    #[test]
    fn without_a_source_nothing_downloads() {
        let s = setup();
        let videos = s.videos(None);
        videos.ensure(true);
        videos.wait();
        assert_eq!(videos.view().state, TrainingVideoState::NotDownloaded);
        assert!(s.events.lock().unwrap().is_empty());
    }

    #[test]
    fn the_list_may_only_name_plain_video_files() {
        assert!(safe_name("t1-welcome.mp4"));
        for bad in [
            "../t1.mp4",
            "a/b.mp4",
            ".hidden.mp4",
            "t1.exe",
            "t1 welcome.mp4",
        ] {
            assert!(!safe_name(bad), "{bad}");
        }
    }

    #[test]
    fn the_published_list_names_each_video_on_the_website() {
        let site = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../site/training");
        let list = published();
        assert_eq!(list.version, VideoManifest::VERSION);
        assert!(!list.videos.is_empty());
        for video in &list.videos {
            let bytes = fs::read(site.join(&video.file)).unwrap();
            assert_eq!(bytes.len() as u64, video.size, "{}", video.file);
            assert_eq!(
                BlobHash::of(&bytes),
                video.blake3,
                "{}: run node training/build.mjs publish",
                video.file
            );
            assert_eq!(format!("{}.mp4", video.id), video.file);
        }
    }
}
