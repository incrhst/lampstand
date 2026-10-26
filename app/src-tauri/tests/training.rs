//! Training services (#190): a fresh install has Training Sessions 0 to 2, each runs
//! start to finish in Present with nothing but what is bundled, and they
//! stay read-only, resettable, out of the repo (so out of sync) and out of
//! CCLI reports.

use std::path::Path;
use std::sync::{Arc, Mutex};

use lampstand_api::{
    Event, LoadedService, OutputRequest, Request, Response, ServiceRequest, ServiceSummary,
};
use lampstand_app::handler::{App, Placement, WindowHost, TRAINING_READ_ONLY};
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
    Harness { app, events }
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

/// Opens each training service for live and presses Next until it stops,
/// checking that every slide and build step of every item came up, with
/// the content its item promises: the whole service runs on what is
/// bundled.
#[test]
fn every_training_service_runs_start_to_finish_in_present() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    add_screen(&mut h);
    let services = training(&mut h);
    assert_eq!(services.len(), 3);
    for summary in &services {
        let service = get(&mut h, summary);
        let loaded = open_for_live(&mut h, summary);
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
            handle(&mut h, Request::Live(Command::Next));
            let Some(cursor) = live_cursor(&h) else {
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
            let fits = matches!(
                (kind, &content),
                (ItemKind::Song { .. }, Some(Content::Lyrics { .. }))
                    | (ItemKind::Scripture { .. }, Some(Content::Scripture { .. }))
                    | (ItemKind::Text { .. }, Some(Content::Text { .. }))
                    | (ItemKind::Deck { .. }, Some(Content::DeckSlide { .. }))
                    | (ItemKind::Media { .. }, Some(Content::Video { .. }))
            );
            assert!(fits, "{}: {kind:?} showed {content:?}", summary.title);
        }
        let expected: Vec<Cursor> = expected.into_iter().map(|(c, _)| c).collect();
        assert_eq!(shown, expected, "{}", summary.title);
        service_request(&mut h, ServiceRequest::Close(summary.id));
    }
}

#[test]
fn the_bundled_videos_play() {
    let dir = tempfile::tempdir().unwrap();
    let h = open(dir.path());
    let blobs = h.app.library().repo().blobs();
    for (name, bytes) in lampstand_app::training::FILES {
        if !name.ends_with(".mp4") {
            continue;
        }
        let path = blobs
            .path(&lampstand_core::id::BlobHash::of(bytes))
            .unwrap();
        let seconds = lampstand_media::duration(&path).unwrap_or(0.0);
        assert!(seconds > 20.0, "{name}: {seconds} s");
        // Lampstand's own decoder plays the picture, start to end...
        let mut video = lampstand_media::Video::open(&path, false).unwrap();
        assert_eq!(video.size(), (1280, 720), "{name}");
        for at in [0.5, seconds / 2.0, seconds - 0.5] {
            let frame = video
                .frame_at_blocking(at, std::time::Duration::from_secs(30))
                .unwrap_or_else(|| panic!("{name}: no frame at {at} s"));
            assert!(!lampstand_media::is_blank(&frame), "{name} at {at} s");
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
