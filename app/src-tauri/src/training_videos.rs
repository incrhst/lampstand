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

/// The manifest last fetched, in `training-videos/`.
const SAVED: &str = "videos.json";

#[derive(Debug, Default, Serialize, Deserialize)]
struct Saved {
    /// When the website last answered, in ms since 1970.
    checked_at: Option<i64>,
    manifest: Option<VideoManifest>,
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
    /// A download was tried since the app started.
    tried: bool,
    /// Bytes done and to do, while downloading.
    progress: Option<(u64, u64)>,
    /// A download ended since `take_changes`.
    ended: bool,
}

impl TrainingVideos {
    /// Reads what an earlier run saved in `dir`. Never touches the network.
    pub fn open(
        dir: PathBuf,
        source: Option<String>,
        library: Arc<Library>,
        emit: EventSink,
    ) -> Self {
        let saved: Saved = fs::read(dir.join(SAVED))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        // A video a newer release names before its website list was
        // fetched comes from the list it was built with.
        let installed = merged(saved.manifest.clone(), published());
        let latest = saved.manifest.unwrap_or_else(published);
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
                    tried: false,
                    progress: None,
                    ended: false,
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
        let listed = shared.latest.videos.iter().any(|v| v.blake3 == *blob);
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
    /// Returns at once; the download runs on its own thread.
    pub fn ensure(&self, recheck: bool) {
        let Some(source) = self.source.clone() else {
            return;
        };
        {
            let mut shared = self.lock();
            if shared.running {
                return;
            }
            let stale = recheck || is_stale(shared.checked_at, now_ms());
            if !stale && missing(&shared.latest, &self.library).is_empty() {
                return;
            }
            shared.running = true;
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
            self.shared.1.notify_all();
        }
    }

    /// `Some` once after each download ends, however it went, so an open
    /// training service is loaded again: its video items play, or say they
    /// could not download. In it, each video whose hash moved, old to new,
    /// so the training services can be pointed at the new one.
    pub fn take_changes(&self) -> Option<Vec<(BlobHash, BlobHash)>> {
        let mut shared = self.lock();
        if !shared.ended {
            return None;
        }
        shared.ended = false;
        let mut moved: Vec<(BlobHash, BlobHash)> = shared
            .installed
            .videos
            .iter()
            .filter_map(|old| {
                let new = shared.latest.video(&old.file)?;
                (new.blake3 != old.blake3).then_some((old.blake3, new.blake3))
            })
            .collect();
        // A video published after the release that names it.
        moved.extend(
            shared
                .latest
                .videos
                .iter()
                .filter(|new| shared.installed.video(&new.file).is_none())
                .map(|new| (unpublished(&new.file), new.blake3)),
        );
        shared.installed = merged(Some(shared.latest.clone()), shared.installed.clone());
        Some(moved)
    }

    /// Waits for a running download to finish (tests).
    pub fn wait(&self) {
        let (lock, done) = &*self.shared;
        let mut shared = lock.lock().unwrap_or_else(|e| e.into_inner());
        while shared.running {
            shared = done.wait(shared).unwrap_or_else(|e| e.into_inner());
        }
    }
}

/// The manifest this build was made with.
pub fn published() -> VideoManifest {
    serde_json::from_str(PUBLISHED).expect("site/training/videos.json is a manifest")
}

/// `newer`, plus any video of `older` it does not name.
fn merged(newer: Option<VideoManifest>, older: VideoManifest) -> VideoManifest {
    let Some(mut out) = newer else {
        return older;
    };
    for video in older.videos {
        if out.video(&video.file).is_none() {
            out.videos.push(video);
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

    fn run(self) {
        match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime.block_on(self.download()),
            Err(e) => eprintln!("lampstand: the training videos cannot download: {e}"),
        }
        {
            let mut shared = self.lock();
            shared.running = false;
            shared.progress = None;
            shared.ended = true;
            self.shared.1.notify_all();
        }
        self.tell();
    }

    async fn download(&self) {
        let client = match reqwest::Client::builder()
            .user_agent(concat!("lampstand/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(30))
            .build()
        {
            Ok(client) => client,
            Err(e) => {
                eprintln!("lampstand: the training videos cannot download: {e}");
                return;
            }
        };
        let now = now_ms();
        let stale = self.recheck || is_stale(self.lock().checked_at, now);
        if stale {
            match fetch_manifest(&client, &self.source).await {
                Ok(fetched) => {
                    let saved = Saved {
                        checked_at: Some(now),
                        manifest: Some(fetched.clone()),
                    };
                    if let Err(e) = save(&self.dir, &saved) {
                        eprintln!("lampstand: the training video list was not saved: {e}");
                    }
                    // The website's list is the one to follow.
                    let mut shared = self.lock();
                    shared.checked_at = Some(now);
                    shared.latest = fetched;
                }
                Err(e) => eprintln!("lampstand: the training video list is unavailable: {e}"),
            }
        }

        let wanted = missing(&self.lock().latest, &self.library);
        if wanted.is_empty() {
            return;
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
            if let Err(e) = result {
                eprintln!("lampstand: training video {}: {e}", video.file);
            }
            self.lock().progress = Some((done, total));
        }
    }
}

fn save(dir: &Path, saved: &Saved) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".tmp-{SAVED}"));
    fs::write(&tmp, serde_json::to_vec_pretty(saved)?)?;
    fs::rename(&tmp, dir.join(SAVED))
}

async fn fetch_manifest(client: &reqwest::Client, source: &str) -> Result<VideoManifest, String> {
    let response = client
        .get(format!("{source}videos.json"))
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.to_string())?;
    let bytes = response.bytes().await.map_err(|e| e.to_string())?;
    let manifest: VideoManifest = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if let Some(bad) = manifest.videos.iter().find(|v| !safe_name(&v.file)) {
        return Err(format!(
            "the list names {:?}, which is not a video file",
            bad.file
        ));
    }
    Ok(manifest)
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
            if written > video.size {
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
        assert_eq!(again.take_changes(), Some(vec![]));
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
            Some(vec![(BlobHash::of(WELCOME), BlobHash::of(NEW))])
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
            Some(vec![(BlobHash::of(NEW), BlobHash::of(NEWER))])
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
        let moved = videos.take_changes().unwrap();
        assert!(moved.contains(&(later, BlobHash::of(LATER))), "{moved:?}");
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
