//! Training services (#190): a fresh install has Training Sessions 0 to 2,
//! each runs start to finish in Present with what is bundled and the
//! training videos the website publishes (served here from
//! `site/training/`), or offline with a message where each video would be,
//! and they stay read-only, resettable, out of the repo (so out of sync)
//! and out of CCLI reports.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use lampstand_api::{
    Event, LoadedService, OutputRequest, Request, Response, ServiceRequest, ServiceSummary,
    TrainingVideoState,
};
use lampstand_app::handler::{App, Placement, WindowHost, TRAINING_READ_ONLY};
use lampstand_app::training_videos::{self, NOT_READY, OFFLINE};
use lampstand_core::output::{BackgroundMode, OutputConfig, OutputId, OutputRole, OutputTarget};
use lampstand_core::program::Content;
use lampstand_core::safe_area::SafeArea;
use lampstand_core::scripture::parse_reference;
use lampstand_core::service::{ItemKind, Service};
use lampstand_layout::Fonts;
use lampstand_library::bundled::BUNDLED;
use lampstand_live::{Command, Cursor};
use lampstand_render::output::DisplayInfo;
use lampstand_render::{OutputCommand, OutputSink};
use lampstand_sync::DocKind;

#[derive(Default)]
struct Sink(Mutex<Vec<OutputCommand>>);

impl OutputSink for Sink {
    fn send(&self, _: OutputId, command: OutputCommand) {
        self.0.lock().unwrap().push(command);
    }
}

struct Host;

impl WindowHost for Host {
    fn displays(&self) -> Vec<DisplayInfo> {
        Vec::new()
    }
    fn open(&mut self, _: OutputId, _: &str, _: Placement) -> Result<(), String> {
        Ok(())
    }
    fn close(&mut self, _: OutputId) {}
}

struct Harness {
    app: App,
    events: Arc<Mutex<Vec<Event>>>,
}

fn open(dir: &Path) -> Harness {
    let events = Arc::new(Mutex::new(Vec::new()));
    let recorded = events.clone();
    let app = App::open(
        dir,
        Fonts::bundled_only(),
        Arc::new(Sink::default()),
        Box::new(Host),
        Arc::new(move |e| recorded.lock().unwrap().push(e)),
    )
    .unwrap();
    let mut h = Harness { app, events };
    // Never the real website from a test.
    h.app.set_training_source(None);
    h
}

/// The website's `training/` folder, as committed.
fn site() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../site/training")
}

/// Serves `site/training/` as the website does.
async fn website() -> wiremock::MockServer {
    let server = wiremock::MockServer::start().await;
    publish(&server, None, None).await;
    server
}

/// Serves `site/training/` on `server`, without `held` (a video not
/// published yet) in the list or beside it, and without the file `gone`
/// although the list names it (a 404).
async fn publish(server: &wiremock::MockServer, held: Option<&str>, gone: Option<&str>) {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, ResponseTemplate};
    server.reset().await;
    let mut list = training_videos::published();
    list.videos.retain(|v| Some(v.file.as_str()) != held);
    Mock::given(method("GET"))
        .and(path("/training/videos.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&list))
        .mount(server)
        .await;
    for video in list.videos.iter().filter(|v| Some(v.file.as_str()) != gone) {
        let file = site().join(&video.file);
        let name = &video.file;
        Mock::given(method("GET"))
            .and(path(format!("/training/{name}")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(std::fs::read(&file).unwrap()))
            .mount(server)
            .await;
    }
}

/// A website nobody answers at.
fn unreachable() -> String {
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    format!("http://127.0.0.1:{port}/training/")
}

fn videos_state(h: &mut Harness) -> TrainingVideoState {
    match service_request(h, ServiceRequest::TrainingVideos) {
        Response::TrainingVideos(v) => v.state,
        other => panic!("{other:?}"),
    }
}

fn handle(h: &mut Harness, request: Request) -> Response {
    let response = h.app.handle(request);
    if let Response::Error { message } = &response {
        panic!("request failed: {message}");
    }
    response
}

fn service_request(h: &mut Harness, request: ServiceRequest) -> Response {
    handle(h, Request::Service(request))
}

fn add_screen(h: &mut Harness) {
    handle(
        h,
        Request::Outputs(OutputRequest::Put(OutputConfig {
            id: OutputId::generate(),
            name: "Projector".into(),
            role: OutputRole::Audience { primary: true },
            target: OutputTarget::Window {
                width: 960,
                height: 540,
            },
            safe_area: SafeArea::default(),
            background: BackgroundMode::Bleed,
            theme: None,
            idle: None,
            enabled: true,
        })),
    );
}

fn training(h: &mut Harness) -> Vec<ServiceSummary> {
    match service_request(h, ServiceRequest::List) {
        Response::Services(all) => all.into_iter().filter(|s| s.training).collect(),
        other => panic!("{other:?}"),
    }
}

fn get(h: &mut Harness, summary: &ServiceSummary) -> Service {
    match service_request(h, ServiceRequest::Get(summary.id)) {
        Response::Service(s) => s,
        other => panic!("{other:?}"),
    }
}

fn open_for_live(h: &mut Harness, summary: &ServiceSummary) -> LoadedService {
    match service_request(h, ServiceRequest::Open(summary.id)) {
        Response::Loaded(loaded) => loaded,
        other => panic!("{other:?}"),
    }
}

fn live_cursor(h: &Harness) -> Option<Cursor> {
    h.events
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find_map(|e| match e {
            Event::Live { live, .. } => Some(*live),
            _ => None,
        })?
}

#[test]
fn a_fresh_install_has_training_0_to_2_after_the_churchs_own_services() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let all = match service_request(&mut h, ServiceRequest::ListChecked) {
        Response::ServiceList(list) => list.services,
        other => panic!("{other:?}"),
    };
    let titles: Vec<&str> = all
        .iter()
        .filter(|s| s.training)
        .map(|s| s.title.as_str())
        .collect();
    assert_eq!(
        titles,
        [
            "Training Session 0: Orientation",
            "Training Session 1: Run a service",
            "Training Session 2: Confident operator"
        ]
    );
    // Nothing of training is a document, so nothing of it can sync.
    let repo = h.app.library().repo();
    for kind in [DocKind::Service, DocKind::Deck, DocKind::Song] {
        for id in repo.list(kind) {
            assert!(
                !all.iter().any(|s| s.id.uuid() == id),
                "training {id} is in the repo"
            );
        }
    }
    assert!(repo.list(DocKind::Deck).is_empty());
    assert!(h.app.library().services().unwrap().is_empty());
}

/// Opens a training service for live and waits for the training videos'
/// download that starts, returning the slides as they are once it has
/// ended (the service is loaded again then).
fn open_and_settle(h: &mut Harness, summary: &ServiceSummary) -> LoadedService {
    let opened = open_for_live(h, summary);
    h.events.lock().unwrap().clear();
    h.app.wait_for_training_videos();
    let reloaded = h.events.lock().unwrap().iter().rev().find_map(|e| match e {
        Event::Loaded(l) if l.service == summary.id => Some(l.clone()),
        _ => None,
    });
    reloaded.unwrap_or(opened)
}

/// Opens each training service for live and presses Next until it stops,
/// checking that every slide and build step of every item came up, with
/// the content its item promises: with the website, the whole service runs
/// and its videos play.
#[tokio::test]
async fn every_training_service_runs_start_to_finish_in_present() {
    let website = website().await;
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    h.app
        .set_training_source(Some(format!("{}/training/", website.uri())));
    add_screen(&mut h);
    assert_eq!(videos_state(&mut h), TrainingVideoState::NotDownloaded);
    let services = training(&mut h);
    assert_eq!(services.len(), 3);
    for summary in &services {
        run_through(&mut h, summary, true);
    }
    // The first one opened downloaded them all.
    assert_eq!(videos_state(&mut h), TrainingVideoState::Ready);
    let asked = website.received_requests().await.unwrap();
    assert_eq!(
        asked.len(),
        training_videos::published().videos.len() + 1,
        "the list and each video, once"
    );
}

/// Offline, with no training video downloaded: every training service
/// still runs start to finish, and each video item says how to get them.
#[test]
fn offline_every_training_service_still_runs_and_its_videos_say_why() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    h.app.set_training_source(Some(unreachable()));
    add_screen(&mut h);
    for summary in &training(&mut h) {
        run_through(&mut h, summary, false);
    }
    assert_eq!(videos_state(&mut h), TrainingVideoState::Offline);
}

/// Downloaded once, the training videos play from then on with no
/// network, after a restart too.
#[tokio::test]
async fn videos_already_downloaded_play_offline() {
    let website = website().await;
    let dir = tempfile::tempdir().unwrap();
    {
        let mut h = open(dir.path());
        h.app
            .set_training_source(Some(format!("{}/training/", website.uri())));
        let t1 = training(&mut h).remove(1);
        get(&mut h, &t1);
        h.app.wait_for_training_videos();
        assert_eq!(videos_state(&mut h), TrainingVideoState::Ready);
    }
    let mut h = open(dir.path());
    h.app.set_training_source(Some(unreachable()));
    add_screen(&mut h);
    assert_eq!(videos_state(&mut h), TrainingVideoState::Ready);
    for summary in &training(&mut h) {
        run_through(&mut h, summary, true);
    }
}

/// A release may name a training video the website has not published yet.
/// Its item says so calmly (nothing about the connection), the rest runs,
/// and once the website lists it, it downloads and plays: no app update.
#[tokio::test]
async fn a_video_not_published_yet_says_so_and_plays_once_it_is() {
    use lampstand_app::training_videos::NOT_READY;
    let website = website().await;
    publish(&website, Some("t1-welcome.mp4"), None).await;
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    h.app
        .set_training_source(Some(format!("{}/training/", website.uri())));
    add_screen(&mut h);
    let t1 = training(&mut h).remove(1);
    let lines = |loaded: &LoadedService, title: &str| -> String {
        let item = loaded.items.iter().find(|i| i.title == title).unwrap();
        item.slides[0].lines.join(" ")
    };
    let loaded = open_and_settle(&mut h, &t1);
    assert_eq!(lines(&loaded, "Welcome.mp4"), NOT_READY);
    assert!(!NOT_READY.contains("internet") && !NOT_READY.contains("connect"));
    // The videos the website has play.
    assert_eq!(
        lines(&loaded, "Going live and moving through slides.mp4"),
        ""
    );
    assert_eq!(videos_state(&mut h), TrainingVideoState::Ready);

    // Published later: Reset training (or the next day's look) finds it.
    publish(&website, None, None).await;
    h.events.lock().unwrap().clear();
    service_request(&mut h, ServiceRequest::ResetTraining);
    h.app.wait_for_training_videos();
    let reloaded = h
        .events
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find_map(|e| match e {
            Event::Loaded(l) => Some(l.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(lines(&reloaded, "Welcome.mp4"), "");
    service_request(&mut h, ServiceRequest::Close(t1.id));
    run_through(&mut h, &t1, true);
}

/// A listed video the website cannot serve is not fetched over and over:
/// with the training service open, ticking, and Present asking for the
/// service again on each reload, the file is asked for at most once.
#[tokio::test]
async fn a_missing_video_is_not_downloaded_again_and_again() {
    let website = wiremock::MockServer::start().await;
    publish(&website, None, Some("t1-welcome.mp4")).await;
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    h.app
        .set_training_source(Some(format!("{}/training/", website.uri())));
    add_screen(&mut h);
    let t1 = training(&mut h).remove(1);
    let loaded = open_and_settle(&mut h, &t1);
    let welcome = loaded
        .items
        .iter()
        .find(|i| i.title == "Welcome.mp4")
        .unwrap();
    assert_eq!(welcome.slides[0].lines.join(" "), OFFLINE);
    for _ in 0..10 {
        h.events.lock().unwrap().clear();
        h.app.tick();
        // What Present does on each Loaded.
        get(&mut h, &t1);
        h.app.wait_for_training_videos();
        let reloaded = h
            .events
            .lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, Event::Loaded(_)));
        assert!(!reloaded, "nothing changed, so nothing is loaded again");
    }
    let asked = website
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path() == "/training/t1-welcome.mp4")
        .count();
    assert_eq!(asked, 1, "the missing video was asked for {asked} times");
    assert_eq!(videos_state(&mut h), TrainingVideoState::Offline);
}

/// Runs one training service from its first slide to its last.
/// `videos_play`: each video item plays its video, or else says
/// [`OFFLINE`].
fn run_through(h: &mut Harness, summary: &ServiceSummary, videos_play: bool) {
    {
        let service = get(h, summary);
        let loaded = open_and_settle(h, summary);
        let mut expected = Vec::new();
        let mut builds = 0;
        for (id, item) in service.ordered() {
            let view = loaded.items.iter().find(|v| v.id == id).unwrap();
            if matches!(item.kind, ItemKind::Header { .. }) {
                assert!(view.slides.is_empty());
                continue;
            }
            assert!(
                !view.slides.is_empty(),
                "{}: {} has nothing to show",
                summary.title,
                view.title
            );
            if let ItemKind::Deck { .. } = item.kind {
                assert!(
                    view.slides.iter().all(|s| !s.notes.is_empty()),
                    "{}: every slide of {} has the trainer's notes",
                    summary.title,
                    view.title
                );
            }
            for (n, slide) in view.slides.iter().enumerate() {
                builds += slide.steps;
                for step in 0..=slide.steps {
                    expected.push((
                        Cursor {
                            item: id,
                            slide: n,
                            step,
                        },
                        item.kind.clone(),
                    ));
                }
            }
        }

        if !summary.title.starts_with("Training Session 0") {
            // The lesson decks' click builds came through the importer.
            assert!(builds >= 3, "{}: {builds} build steps", summary.title);
        }

        h.events.lock().unwrap().clear();
        let mut shown = Vec::new();
        for _ in 0..expected.len() + 5 {
            handle(h, Request::Live(Command::Next));
            let Some(cursor) = live_cursor(h) else {
                continue;
            };
            if shown.last() == Some(&cursor) {
                break;
            }
            shown.push(cursor);
            let (_, kind) = expected
                .iter()
                .find(|(c, _)| *c == cursor)
                .unwrap_or_else(|| panic!("{}: unexpected {cursor:?}", summary.title));
            let content = h.app.program().content.clone();
            // A video the website hasn't published yet (a lesson's video can
            // follow a release) says so instead of playing.
            let published = |blob| {
                training_videos::published()
                    .videos
                    .iter()
                    .any(|v| v.blake3 == blob)
            };
            let fits = match (kind, &content) {
                (ItemKind::Media { blob, .. }, Some(Content::Video { blob: playing, .. })) => {
                    videos_play && published(*blob) && blob == playing
                }
                (ItemKind::Media { blob, .. }, Some(Content::Text { slide })) => {
                    let said = slide.lines.join(" ");
                    if published(*blob) {
                        !videos_play && said == OFFLINE
                    } else {
                        said == NOT_READY || (!videos_play && said == OFFLINE)
                    }
                }
                _ => matches!(
                    (kind, &content),
                    (ItemKind::Song { .. }, Some(Content::Lyrics { .. }))
                        | (ItemKind::Scripture { .. }, Some(Content::Scripture { .. }))
                        | (ItemKind::Text { .. }, Some(Content::Text { .. }))
                        | (ItemKind::Deck { .. }, Some(Content::DeckSlide { .. }))
                ),
            };
            assert!(fits, "{}: {kind:?} showed {content:?}", summary.title);
        }
        let expected: Vec<Cursor> = expected.into_iter().map(|(c, _)| c).collect();
        assert_eq!(shown, expected, "{}", summary.title);
        service_request(h, ServiceRequest::Close(summary.id));
    }
}

/// The training videos the website publishes, as `site/training/` holds
/// them, are the videos the manifest names and play in Lampstand.
#[test]
fn the_published_videos_play() {
    let list = training_videos::published();
    assert!(!list.videos.is_empty());
    for entry in &list.videos {
        let name = &entry.file;
        let path = site().join(name);
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(bytes.len() as u64, entry.size, "{name}");
        assert_eq!(
            lampstand_core::id::BlobHash::of(&bytes),
            entry.blake3,
            "{name}: run node training/build.mjs publish"
        );
        let seconds = lampstand_media::duration(&path).unwrap_or(0.0);
        assert!(seconds > 20.0, "{name}: {seconds} s");
        // Lampstand's own decoder plays the picture, start to end...
        let mut video = lampstand_media::Video::open(&path, false).unwrap();
        assert_eq!(video.size(), (1280, 720), "{name}");
        // A sample can land on a fade between scenes, so a frame a second
        // earlier counts too.
        for at in [0.5, seconds / 2.0, seconds - 0.5] {
            let shows = [at, (at - 1.0).max(0.0)].into_iter().any(|t| {
                let frame = video
                    .frame_at_blocking(t, std::time::Duration::from_secs(30))
                    .unwrap_or_else(|| panic!("{name}: no frame at {t} s"));
                !lampstand_media::is_blank(&frame)
            });
            assert!(shows, "{name} at {at} s");
        }
        // ...and the narration.
        let pcm = lampstand_media::load_sound(&path).unwrap();
        let loudest = pcm.samples.iter().fold(0f32, |m, s| m.max(s.abs()));
        assert!(loudest > 0.05, "{name} has narration ({loudest})");
    }
}

#[test]
fn training_is_read_only_in_plan_but_quick_add_works_in_present_until_reset() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    add_screen(&mut h);
    let t2 = training(&mut h).remove(2);
    let before = get(&mut h, &t2);
    let refused = |h: &mut Harness, request: ServiceRequest| {
        assert_eq!(
            h.app.handle(Request::Service(request)),
            Response::Error {
                message: TRAINING_READ_ONLY.into()
            }
        );
    };
    let verse = ItemKind::Scripture {
        passage: parse_reference("Rom 8:28").unwrap(),
        translation: BUNDLED[0].id,
    };
    refused(
        &mut h,
        ServiceRequest::Rename {
            service: t2.id,
            title: "Mine".into(),
        },
    );
    refused(&mut h, ServiceRequest::Delete(t2.id));
    // Not open for live: Plan cannot add to it.
    refused(
        &mut h,
        ServiceRequest::InsertItem {
            service: t2.id,
            index: 3,
            kind: verse.clone(),
        },
    );

    // Open for live, Quick add puts Romans 8:28 in, for practice.
    open_for_live(&mut h, &t2);
    let added = match service_request(
        &mut h,
        ServiceRequest::InsertItem {
            service: t2.id,
            index: 8,
            kind: verse,
        },
    ) {
        Response::Service(s) => s,
        other => panic!("{other:?}"),
    };
    assert_eq!(added.items.len(), before.items.len() + 1);
    assert!(h.app.library().repo().list(DocKind::Service).is_empty());
    refused(
        &mut h,
        ServiceRequest::MoveItem {
            service: t2.id,
            item: *added.items.keys().next().unwrap(),
            index: 0,
        },
    );

    service_request(&mut h, ServiceRequest::ResetTraining);
    assert_eq!(get(&mut h, &t2), before);
    assert!(!h.app.library().counts_for_ccli(t2.id));
}
