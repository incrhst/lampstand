//! Crash recovery (T57): a service killed mid-slide comes back on the same
//! slide, on its window output, in under 5 seconds.
//!
//! The kill test runs this test binary a second time as the "app": the
//! child opens a service on a rehearsal window, takes a slide live, says
//! so, and waits to be killed with `kill -9`. The parent then starts the
//! app again on the same data folder, as a relaunch after a crash would.

use std::path::Path;
use std::process::{Command as Process, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use lampstand_api::{
    Event, LibraryRequest, OutputRequest, Request, Response, ServiceRequest, SoundRequest,
};
use lampstand_app::handler::{App, Placement, WindowHost};
use lampstand_core::output::{BackgroundMode, OutputConfig, OutputId, OutputRole, OutputTarget};
use lampstand_core::program::{Content, Overlay};
use lampstand_core::safe_area::SafeArea;
use lampstand_core::service::{ItemKind, Service, Sound, SoundStart};
use lampstand_layout::Fonts;
use lampstand_live::{Command, Cursor};
use lampstand_render::output::DisplayInfo;
use lampstand_render::{OutputCommand, OutputSink};

/// Set to the data folder when this binary runs as the child.
const CHILD: &str = "LAMPSTAND_JOURNAL_CHILD";
/// The child writes this file, in the data folder, once its slide is live.
const READY: &str = "ready.json";

#[derive(Default)]
struct Sink(Mutex<Vec<(OutputId, OutputCommand)>>);

impl OutputSink for Sink {
    fn send(&self, output: OutputId, command: OutputCommand) {
        self.0.lock().unwrap().push((output, command));
    }
}

#[derive(Clone, Default)]
struct Host(Arc<Mutex<Vec<OutputId>>>);

impl WindowHost for Host {
    fn displays(&self) -> Vec<DisplayInfo> {
        Vec::new()
    }
    fn open(&mut self, output: OutputId, _: &str, _: Placement) -> Result<(), String> {
        self.0.lock().unwrap().push(output);
        Ok(())
    }
    fn close(&mut self, output: OutputId) {
        self.0.lock().unwrap().retain(|o| *o != output);
    }
}

struct Harness {
    app: App,
    sink: Arc<Sink>,
    host: Host,
    events: Arc<Mutex<Vec<Event>>>,
}

fn open(dir: &Path) -> Harness {
    let sink = Arc::new(Sink::default());
    let host = Host::default();
    let events = Arc::new(Mutex::new(Vec::new()));
    let recorded = events.clone();
    let app = App::open(
        dir,
        Fonts::bundled_only(),
        sink.clone(),
        Box::new(host.clone()),
        Arc::new(move |e| recorded.lock().unwrap().push(e)),
    )
    .unwrap();
    Harness {
        app,
        sink,
        host,
        events,
    }
}

fn ok(response: Response) -> Response {
    if let Response::Error { message } = &response {
        panic!("request failed: {message}");
    }
    response
}

fn window_output() -> OutputConfig {
    OutputConfig {
        id: OutputId::generate(),
        name: "Rehearsal".into(),
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
    }
}

/// A window output and a service of Amazing Grace, opened for live.
/// Returns the output and the song's cursor at slide 0.
fn start_service(h: &mut Harness) -> (OutputId, Cursor) {
    let output = window_output();
    let id = output.id;
    ok(h.app.handle(Request::Outputs(OutputRequest::Put(output))));
    let song = match ok(h.app.handle(Request::Library(LibraryRequest::SearchSongs {
        query: "Amazing Grace".into(),
        limit: 1,
    }))) {
        Response::Songs(hits) => hits[0].song,
        other => panic!("{other:?}"),
    };
    let service: Service = match ok(h.app.handle(Request::Service(ServiceRequest::Create {
        title: "Sunday".into(),
        starts_at: None,
        time_zone: None,
    }))) {
        Response::Service(s) => s,
        other => panic!("{other:?}"),
    };
    ok(h.app.handle(Request::Service(ServiceRequest::InsertItem {
        service: service.id,
        index: 0,
        kind: ItemKind::Song {
            song,
            arrangement: None,
        },
    })));
    let loaded = match ok(h
        .app
        .handle(Request::Service(ServiceRequest::Open(service.id))))
    {
        Response::Loaded(loaded) => loaded,
        other => panic!("{other:?}"),
    };
    assert!(loaded.items[0].slides.len() >= 4, "{loaded:?}");
    let cursor = Cursor {
        item: loaded.items[0].id,
        slide: 0,
        step: 0,
    };
    (id, cursor)
}

fn at(cursor: Cursor, slide: usize) -> Cursor {
    Cursor { slide, ..cursor }
}

fn last_live(h: &Harness) -> (Option<Cursor>, Option<Cursor>, Overlay) {
    h.events
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find_map(|e| match e {
            Event::Live {
                live,
                preview,
                overlay,
                ..
            } => Some((*live, *preview, *overlay)),
            _ => None,
        })
        .expect("no Live event")
}

fn shown_on(h: &Harness, output: OutputId) -> usize {
    h.sink
        .0
        .lock()
        .unwrap()
        .iter()
        .filter(|(id, c)| *id == output && matches!(c, OutputCommand::Show { .. }))
        .count()
}

/// The "app" the kill test kills. Does nothing unless run by it.
#[test]
#[ignore = "run by kill_9_mid_service_restores_the_live_slide"]
fn journal_child() {
    let Some(dir) = std::env::var_os(CHILD) else {
        return;
    };
    let mut h = open(Path::new(&dir));
    let (output, cursor) = start_service(&mut h);
    ok(h.app.handle(Request::Live(Command::Show(at(cursor, 2)))));
    ok(h.app.handle(Request::Live(Command::Preview(at(cursor, 3)))));
    let lyrics = h.app.program().content.clone();
    // libtest may capture stdout, so say so in a file, renamed into place
    // so the parent never reads half of it.
    let dir = Path::new(&dir);
    let ready = serde_json::to_vec(&(output, cursor, lyrics)).unwrap();
    std::fs::write(dir.join("ready.tmp"), ready).unwrap();
    std::fs::rename(dir.join("ready.tmp"), dir.join(READY)).unwrap();
    loop {
        std::thread::sleep(Duration::from_secs(60));
    }
}

#[test]
fn kill_9_mid_service_restores_the_live_slide() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = Process::new(std::env::current_exe().unwrap())
        .args(["journal_child", "--exact", "--ignored", "--test-threads=1"])
        .env(CHILD, dir.path())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let waiting = Instant::now();
    let ready = loop {
        if let Ok(bytes) = std::fs::read(dir.path().join(READY)) {
            break bytes;
        }
        if let Some(status) = child.try_wait().unwrap() {
            panic!("the child exited before going live: {status}");
        }
        if waiting.elapsed() > Duration::from_secs(120) {
            let _ = child.kill();
            panic!("the child did not go live");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let (output, cursor, lyrics): (OutputId, Cursor, Option<Content>) =
        serde_json::from_slice(&ready).unwrap();
    assert!(matches!(lyrics, Some(Content::Lyrics { .. })), "{lyrics:?}");

    // kill -9: no destructors, no clean quit.
    #[cfg(unix)]
    {
        let status = Process::new("kill")
            .args(["-9", &child.id().to_string()])
            .status()
            .unwrap();
        assert!(status.success());
    }
    #[cfg(not(unix))]
    child.kill().unwrap();
    let status = child.wait().unwrap();
    assert!(!status.success(), "{status:?}");

    let started = Instant::now();
    let h = open(dir.path());
    let restored = started.elapsed();
    assert_eq!(h.app.program().content, lyrics, "the same slide is on air");
    assert_eq!(
        last_live(&h),
        (Some(at(cursor, 2)), Some(at(cursor, 3)), Overlay::None)
    );
    assert_eq!(*h.host.0.lock().unwrap(), [output], "its window is open");
    assert!(shown_on(&h, output) > 0, "and shows the slide");
    assert!(
        restored < Duration::from_secs(5),
        "restored in {restored:?}"
    );
}

#[test]
fn overlay_and_cleared_text_come_back() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let (_, cursor) = start_service(&mut h);
    ok(h.app.handle(Request::Live(Command::Show(at(cursor, 1)))));
    ok(h.app.handle(Request::Live(Command::ToggleBlack)));
    ok(h.app.handle(Request::Live(Command::ClearText)));
    // Dropped without `closing`, as a crash would.
    drop(h);

    let h = open(dir.path());
    assert_eq!(h.app.program().overlay, Overlay::Black);
    assert_eq!(h.app.program().content, None);
    assert_eq!(last_live(&h).0, Some(at(cursor, 1)));
}

/// What the shell does at start (`serve_remotes`): speakers' phones are
/// kept only for the service the journal restored.
fn start_remotes(dir: &Path, h: &Harness) -> lampstand_server::RemoteHub {
    let hub = lampstand_app::remote::open_hub(dir).unwrap();
    lampstand_app::remote::end_speakers(&hub, h.app.open_service());
    hub
}

#[test]
fn a_speakers_phone_survives_a_crash_restore_but_not_a_quit() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let (_, cursor) = start_service(&mut h);
    let service = h.app.open_service().unwrap();
    ok(h.app.handle(Request::Live(Command::Show(cursor))));
    let token = {
        let hub = start_remotes(dir.path(), &h);
        let pin = hub.start_pairing_as(lampstand_server::RemoteKind::Speaker {
            service: service.to_string(),
            item: cursor.item.to_string(),
            title: "Amazing Grace".into(),
        });
        hub.pair("192.168.1.30".parse().unwrap(), &pin.pin, "Pastor")
            .unwrap()
            .0
    };
    // A crash mid-service: the journal brings the service back, and the
    // speaker carries on.
    drop(h);
    let mut h = open(dir.path());
    assert_eq!(h.app.open_service(), Some(service));
    assert!(start_remotes(dir.path(), &h).session(&token).is_some());
    // A clean quit restores nothing, so the phone stops; opening the same
    // service later does not bring it back.
    h.app.closing();
    drop(h);
    let mut h = open(dir.path());
    assert_eq!(h.app.open_service(), None);
    assert!(start_remotes(dir.path(), &h).session(&token).is_none());
    ok(h.app
        .handle(Request::Service(ServiceRequest::Open(service))));
    assert!(start_remotes(dir.path(), &h).session(&token).is_none());
}

#[test]
fn a_clean_quit_or_an_idle_app_restores_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let (_, cursor) = start_service(&mut h);
    ok(h.app.handle(Request::Live(Command::Show(at(cursor, 1)))));
    h.app.closing();
    drop(h);
    let h = open(dir.path());
    assert_eq!(h.app.program().content, None);
    drop(h);

    // Opened but never taken live: nothing to restore either.
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    start_service(&mut h);
    drop(h);
    let h = open(dir.path());
    assert!(h
        .events
        .lock()
        .unwrap()
        .iter()
        .all(|e| !matches!(e, Event::Loaded(_))));
}

#[test]
fn a_journal_older_than_ten_minutes_is_not_restored() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let (_, cursor) = start_service(&mut h);
    ok(h.app.handle(Request::Live(Command::Show(at(cursor, 1)))));
    drop(h);
    // Age the journal by rewriting its timestamp.
    let path = dir.path().join("journal.json");
    let mut journal: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let at_ms = journal["at"].as_i64().unwrap();
    journal["at"] = (at_ms - 11 * 60 * 1000).into();
    std::fs::write(&path, serde_json::to_vec(&journal).unwrap()).unwrap();

    let h = open(dir.path());
    assert_eq!(h.app.program().content, None);
}

/// The service of `start_service` with walk-in music that plays on its own
/// after the song: the music's cursor.
fn add_music(h: &mut Harness, song: Cursor) -> Cursor {
    let file = h
        .app
        .library()
        .repo()
        .blobs()
        .put(b"not really music")
        .unwrap();
    let service = match ok(h.app.handle(Request::Service(ServiceRequest::List))) {
        Response::Services(list) => list[0].id,
        other => panic!("{other:?}"),
    };
    let s = match ok(h.app.handle(Request::Service(ServiceRequest::InsertItem {
        service,
        index: 1,
        kind: ItemKind::Sound {
            sound: Sound {
                name: "Walk-in music".into(),
                file,
                length_ms: Some(30_000),
                start: SoundStart::OnItsOwn,
                keep_off_stream: false,
            },
        },
    }))) {
        Response::Service(s) => s,
        other => panic!("{other:?}"),
    };
    let (item, _) = s
        .ordered()
        .into_iter()
        .find(|(id, _)| *id != song.item)
        .unwrap();
    Cursor {
        item,
        slide: 0,
        step: 0,
    }
}

fn last_shown(h: &Harness) -> Option<Cursor> {
    h.events
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find_map(|e| match e {
            Event::Live { shown, .. } => Some(*shown),
            _ => None,
        })?
}

/// Neither playing nor tried: a cue would have left a problem (the file is
/// not sound) or a sound playing.
fn no_sound_cued(h: &mut Harness) {
    match ok(h.app.handle(Request::Sound(SoundRequest::Status))) {
        Response::Sound(v) => assert!(v.playing.is_none() && v.problem.is_none(), "{v:?}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_crash_while_music_is_live_brings_back_the_slide_on_the_screens_and_no_sound() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let (_, song) = start_service(&mut h);
    let music = add_music(&mut h, song);
    ok(h.app.handle(Request::Live(Command::Show(at(song, 1)))));
    let lyrics = h.app.program().content.clone();
    let background = h.app.program().background.clone();
    ok(h.app.handle(Request::Live(Command::Show(music))));
    assert_eq!(
        h.app.program().content,
        lyrics,
        "the screens keep the slide"
    );
    drop(h);

    let mut h = open(dir.path());
    assert_eq!(h.app.program().content, lyrics, "the same slide is on air");
    assert_eq!(h.app.program().background, background);
    assert_eq!(last_live(&h).0, Some(music), "with the music live");
    assert_eq!(last_shown(&h), Some(at(song, 1)));
    no_sound_cued(&mut h);
}

#[test]
fn a_journal_from_before_shown_was_kept_brings_back_the_slide_before_the_music() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let (_, song) = start_service(&mut h);
    let music = add_music(&mut h, song);
    ok(h.app.handle(Request::Live(Command::Show(song))));
    ok(h.app.handle(Request::Live(Command::NextItem)));
    assert_eq!(last_live(&h).0, Some(music));
    drop(h);
    // An older journal has no `shown`.
    let path = dir.path().join("journal.json");
    let mut journal: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert!(journal.as_object_mut().unwrap().remove("shown").is_some());
    std::fs::write(&path, serde_json::to_vec(&journal).unwrap()).unwrap();

    let mut h = open(dir.path());
    assert_eq!(last_live(&h).0, Some(music));
    // The slide Previous reaches from the music: the song's last.
    assert!(matches!(
        h.app.program().content,
        Some(Content::Lyrics { .. })
    ));
    assert_eq!(last_shown(&h).map(|c| c.item), Some(song.item));
    assert!(last_shown(&h).is_some_and(|c| c.slide > 0));
    no_sound_cued(&mut h);
}
