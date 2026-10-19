//! The request handlers end to end, without Tauri or a GPU: a recording
//! `OutputSink` stands in for the render thread and a fake `WindowHost`
//! for the output windows.

use std::path::Path;
use std::sync::{Arc, Mutex};

use lampstand_api::{
    Event, LibraryRequest, LoadedService, OutputRequest, Request, Response, ServiceRequest,
};
use lampstand_app::handler::{App, Placement, WindowHost};
use lampstand_core::output::{
    BackgroundMode, DisplayMatch, OutputConfig, OutputId, OutputRole, OutputTarget,
};
use lampstand_core::program::{Content, Overlay};
use lampstand_core::safe_area::SafeArea;
use lampstand_core::scene::{NodeKind, Scene};
use lampstand_core::service::{ItemKind, Service};
use lampstand_core::song::SongId;
use lampstand_layout::Fonts;
use lampstand_live::Command;
use lampstand_render::output::DisplayInfo;
use lampstand_render::{OutputCommand, OutputSink};

#[derive(Default)]
struct Sink(Mutex<Vec<(OutputId, OutputCommand)>>);

impl OutputSink for Sink {
    fn send(&self, output: OutputId, command: OutputCommand) {
        self.0.lock().unwrap().push((output, command));
    }
}

impl Sink {
    fn take(&self) -> Vec<(OutputId, OutputCommand)> {
        std::mem::take(&mut self.0.lock().unwrap())
    }
}

#[derive(Default)]
struct Windows {
    open: Vec<(OutputId, Placement)>,
    closed: Vec<OutputId>,
    displays: Vec<DisplayInfo>,
}

#[derive(Clone, Default)]
struct Host(Arc<Mutex<Windows>>);

impl WindowHost for Host {
    fn displays(&self) -> Vec<DisplayInfo> {
        self.0.lock().unwrap().displays.clone()
    }
    fn open(&mut self, output: OutputId, _: &str, placement: Placement) -> Result<(), String> {
        self.0.lock().unwrap().open.push((output, placement));
        Ok(())
    }
    fn close(&mut self, output: OutputId) {
        self.0.lock().unwrap().closed.push(output);
    }
}

struct Harness {
    app: App,
    sink: Arc<Sink>,
    host: Host,
    events: Arc<Mutex<Vec<Event>>>,
}

fn open(dir: &Path) -> Harness {
    open_with(dir, Host::default())
}

fn open_with(dir: &Path, host: Host) -> Harness {
    let sink = Arc::new(Sink::default());
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
        enabled: true,
    }
}

fn search(h: &mut Harness, query: &str) -> Vec<(SongId, String)> {
    match ok(h.app.handle(Request::Library(LibraryRequest::SearchSongs {
        query: query.into(),
        limit: 10,
    }))) {
        Response::Songs(hits) => hits.into_iter().map(|h| (h.song, h.title)).collect(),
        other => panic!("{other:?}"),
    }
}

fn create_service(h: &mut Harness, title: &str) -> Service {
    match ok(h.app.handle(Request::Service(ServiceRequest::Create {
        title: title.into(),
    }))) {
        Response::Service(s) => s,
        other => panic!("{other:?}"),
    }
}

fn insert_song(h: &mut Harness, service: &Service, song: SongId) -> Service {
    match ok(h.app.handle(Request::Service(ServiceRequest::InsertItem {
        service: service.id,
        index: usize::MAX,
        kind: ItemKind::Song {
            song,
            arrangement: None,
        },
    }))) {
        Response::Service(s) => s,
        other => panic!("{other:?}"),
    }
}

fn open_service(h: &mut Harness, service: &Service) -> LoadedService {
    match ok(h
        .app
        .handle(Request::Service(ServiceRequest::Open(service.id))))
    {
        Response::Loaded(loaded) => loaded,
        other => panic!("{other:?}"),
    }
}

/// Every text run in a scene, joined.
fn scene_text(scene: &Scene) -> String {
    fn walk(nodes: &[lampstand_core::scene::Node], out: &mut String) {
        for node in nodes {
            match &node.kind {
                NodeKind::Text(t) => {
                    for p in &t.paragraphs {
                        for r in &p.runs {
                            out.push_str(&r.text);
                        }
                        out.push('\n');
                    }
                }
                NodeKind::Group(children) => walk(children, out),
                _ => {}
            }
        }
    }
    let mut out = String::new();
    walk(&scene.nodes, &mut out);
    out
}

fn shown_scenes(sink: &Sink) -> Vec<(OutputId, Arc<Scene>)> {
    sink.take()
        .into_iter()
        .filter_map(|(id, c)| match c {
            OutputCommand::Show { scene, .. } => Some((id, scene)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_fresh_library_has_the_seed_hymns_once() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let titles: Vec<String> = search(&mut h, "").into_iter().map(|(_, t)| t).collect();
    assert_eq!(titles, ["Amazing Grace", "Holy, Holy, Holy"]);
    drop(h);
    // Reopening does not seed again.
    let mut h = open(dir.path());
    assert_eq!(search(&mut h, "").len(), 2);
}

#[test]
fn search_finds_a_seed_hymn_by_a_lyric_with_a_typo() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let hits = search(&mut h, "wretch");
    assert_eq!(hits.first().map(|(_, t)| t.as_str()), Some("Amazing Grace"));
    let hits = search(&mut h, "amazng");
    assert_eq!(hits.first().map(|(_, t)| t.as_str()), Some("Amazing Grace"));
}

#[test]
fn import_files_reads_songs_and_bibles_and_reports_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let fixtures =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/lampstand-import/tests/fixtures");
    let paths = [
        fixtures.join("songselect/lamp-for-the-path.usr"),
        fixtures.join("osis/kjv-container.osis.xml"),
        fixtures.join("openlyrics/readme.txt"),
    ]
    .iter()
    .map(|p| p.display().to_string())
    .collect();
    let message = match ok(h
        .app
        .handle(Request::Library(LibraryRequest::ImportFiles { paths })))
    {
        Response::Text(t) => t,
        other => panic!("{other:?}"),
    };
    assert!(message.starts_with("Imported 1 song"), "{message}");
    assert!(message.contains("skipped 1"), "{message}");
    assert_eq!(search(&mut h, "").len(), 3);
    let translations = match ok(h
        .app
        .handle(Request::Library(LibraryRequest::ListTranslations)))
    {
        Response::Translations(t) => t,
        other => panic!("{other:?}"),
    };
    assert_eq!(translations.len(), 1);
    // The Bible is usable straight away.
    let passage = h
        .app
        .handle(Request::Library(LibraryRequest::LookupPassage {
            reference: "Gen 1:1".into(),
            translation: translations[0].id,
        }));
    assert!(
        matches!(passage, Response::Passage { ref verses, .. } if verses.len() == 1),
        "{passage:?}"
    );
    let events = h.events.lock().unwrap();
    assert!(matches!(
        events
            .iter()
            .rev()
            .find(|e| matches!(e, Event::ImportProgress { .. })),
        Some(Event::ImportProgress {
            done: 3,
            total: 3,
            ..
        })
    ));
}

#[test]
fn create_insert_open_next_shows_the_first_verse_on_the_window_output() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let output = window_output();
    ok(h.app
        .handle(Request::Outputs(OutputRequest::Put(output.clone()))));
    assert_eq!(
        h.host.0.lock().unwrap().open,
        [(
            output.id,
            Placement::Window {
                width: 960,
                height: 540
            }
        )]
    );

    let service = create_service(&mut h, "Sunday");
    let (song, _) = search(&mut h, "amazing grace")[0].clone();
    let service = insert_song(&mut h, &service, song);
    assert_eq!(service.items.len(), 1);

    let listed = match ok(h.app.handle(Request::Service(ServiceRequest::List))) {
        Response::Services(s) => s,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        listed.iter().map(|s| s.title.as_str()).collect::<Vec<_>>(),
        ["Sunday"]
    );

    let loaded = open_service(&mut h, &service);
    assert_eq!(loaded.items.len(), 1);
    assert_eq!(loaded.items[0].title, "Amazing Grace");
    // Four verses of four lines, at the starter theme's four lines a slide.
    assert_eq!(loaded.items[0].slides.len(), 4);
    assert_eq!(loaded.items[0].slides[0].label.as_deref(), Some("V1"));
    h.sink.take();

    ok(h.app.handle(Request::Live(Command::Next)));
    let program = h.app.program().clone();
    let Some(Content::Lyrics { song: shown, slide }) = &program.content else {
        panic!("{program:?}");
    };
    assert_eq!(*shown, song);
    assert_eq!(slide.lines[0], "Amazing grace! How sweet the sound");
    assert_eq!(slide.footer.as_deref(), Some("Public Domain"));

    let shown = shown_scenes(&h.sink);
    assert_eq!(shown.len(), 1, "one Show for the one enabled output");
    let (id, scene) = &shown[0];
    assert_eq!(*id, output.id);
    assert_eq!((scene.width, scene.height), (960.0, 540.0));
    assert!(scene_text(scene).contains("That saved a wretch like me!"));

    let live = h.events.lock().unwrap().last().cloned();
    assert!(
        matches!(live, Some(Event::Live { live: Some(c), idle: false, overlay: Overlay::None, .. }) if c.slide == 0),
        "{live:?}"
    );
}

#[test]
fn next_previous_and_black_drive_the_output() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let output = window_output();
    ok(h.app.handle(Request::Outputs(OutputRequest::Put(output))));
    let service = create_service(&mut h, "Sunday");
    let (song, _) = search(&mut h, "holy")[0].clone();
    let service = insert_song(&mut h, &service, song);
    open_service(&mut h, &service);

    let line = |h: &Harness| match &h.app.program().content {
        Some(Content::Lyrics { slide, .. }) => slide.lines[0].clone(),
        other => panic!("{other:?}"),
    };
    ok(h.app.handle(Request::Live(Command::Next)));
    ok(h.app.handle(Request::Live(Command::Next)));
    assert!(line(&h).starts_with("Holy, holy, holy! All the saints"));
    ok(h.app.handle(Request::Live(Command::Previous)));
    assert!(line(&h).starts_with("Holy, holy, holy! Lord God"));
    h.sink.take();

    ok(h.app.handle(Request::Live(Command::ToggleBlack)));
    assert_eq!(h.app.program().overlay, Overlay::Black);
    let shown = shown_scenes(&h.sink);
    assert_eq!(shown.len(), 1);
    assert!(scene_text(&shown[0].1).is_empty(), "black covers the text");

    // Nothing changed, so nothing is composed or sent again.
    ok(h.app.handle(Request::Outputs(OutputRequest::List)));
    assert!(h.sink.take().is_empty());

    ok(h.app.handle(Request::Live(Command::ClearAll)));
    assert_eq!(h.app.program().overlay, Overlay::None);
    assert!(h.app.program().content.is_none());
}

#[test]
fn disabled_outputs_get_no_window_and_closing_one_disables_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let mut output = window_output();
    output.enabled = false;
    ok(h.app
        .handle(Request::Outputs(OutputRequest::Put(output.clone()))));
    assert!(h.host.0.lock().unwrap().open.is_empty());

    output.enabled = true;
    ok(h.app
        .handle(Request::Outputs(OutputRequest::Put(output.clone()))));
    assert_eq!(h.host.0.lock().unwrap().open.len(), 1);

    h.app.output_closed(output.id);
    let listed = match ok(h.app.handle(Request::Outputs(OutputRequest::List))) {
        Response::Outputs(o) => o,
        other => panic!("{other:?}"),
    };
    assert!(!listed[0].enabled);
    assert_eq!(h.host.0.lock().unwrap().closed, [output.id]);
}

#[test]
fn outputs_are_device_local_and_reopen_their_windows() {
    let dir = tempfile::tempdir().unwrap();
    let output = window_output();
    {
        let mut h = open(dir.path());
        ok(h.app
            .handle(Request::Outputs(OutputRequest::Put(output.clone()))));
    }
    assert!(dir.path().join("outputs.json").exists());
    let h = open(dir.path());
    assert_eq!(h.host.0.lock().unwrap().open.len(), 1);
}

#[test]
fn a_display_output_waits_for_its_display() {
    let dir = tempfile::tempdir().unwrap();
    let host = Host::default();
    let mut h = open_with(dir.path(), host.clone());
    let projector = DisplayInfo {
        name: "Projector".into(),
        width: 1920,
        height: 1080,
        x: 1512,
        y: 0,
    };
    let mut output = window_output();
    output.target = OutputTarget::Display(DisplayMatch {
        name: "Projector".into(),
        width: 1920,
        height: 1080,
        x: 1512,
        y: 0,
    });
    ok(h.app
        .handle(Request::Outputs(OutputRequest::Put(output.clone()))));
    assert!(host.0.lock().unwrap().open.is_empty(), "not connected yet");

    host.0.lock().unwrap().displays.push(projector.clone());
    h.app.displays_changed();
    assert_eq!(
        host.0.lock().unwrap().open,
        [(output.id, Placement::Display(projector))]
    );
    let displays = ok(h.app.handle(Request::Outputs(OutputRequest::ListDisplays)));
    assert!(matches!(displays, Response::Displays(d) if d.len() == 1));
}

#[test]
fn the_real_window_size_is_what_gets_composed() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let output = window_output();
    ok(h.app
        .handle(Request::Outputs(OutputRequest::Put(output.clone()))));
    let service = create_service(&mut h, "Sunday");
    let (song, _) = search(&mut h, "amazing")[0].clone();
    let service = insert_song(&mut h, &service, song);
    open_service(&mut h, &service);
    ok(h.app.handle(Request::Live(Command::Next)));
    h.sink.take();

    // A Retina window reports twice the pixels.
    h.app.output_resized(output.id, 1920, 1080);
    let shown = shown_scenes(&h.sink);
    assert_eq!(shown.len(), 1);
    assert_eq!((shown[0].1.width, shown[0].1.height), (1920.0, 1080.0));
}

#[test]
fn editing_the_open_service_reloads_it_without_changing_what_is_live() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let service = create_service(&mut h, "Sunday");
    let hits = search(&mut h, "");
    let service = insert_song(&mut h, &service, hits[0].0);
    open_service(&mut h, &service);
    ok(h.app.handle(Request::Live(Command::Next)));
    let before = h.app.program().content.clone();

    insert_song(&mut h, &service, hits[1].0);
    assert_eq!(h.app.program().content, before);
    let reloaded = h.events.lock().unwrap().iter().rev().find_map(|e| match e {
        Event::Loaded(l) => Some(l.items.len()),
        _ => None,
    });
    assert_eq!(reloaded, Some(2));
}

#[test]
fn unbuilt_areas_answer_with_an_error_not_a_panic() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let mut obs = window_output();
    obs.target = OutputTarget::BrowserSource {
        width: 1920,
        height: 1080,
    };
    for request in [
        Request::Outputs(OutputRequest::Put(obs)),
        Request::Outputs(OutputRequest::BrowserSourceUrl(OutputId::generate())),
        Request::Team(lampstand_api::TeamRequest::Status),
        Request::Service(ServiceRequest::Open(
            lampstand_core::service::ServiceId::generate(),
        )),
    ] {
        assert!(matches!(h.app.handle(request), Response::Error { .. }));
    }
}

#[test]
fn importing_the_same_songs_twice_does_not_duplicate_them() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let osb = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/lampstand-import/tests/fixtures/opensong_backup/hymns.osb")
        .display()
        .to_string();
    let import =
        |h: &mut Harness| match ok(h.app.handle(Request::Library(LibraryRequest::ImportFiles {
            paths: vec![osb.clone()],
        }))) {
            Response::Text(t) => t,
            other => panic!("{other:?}"),
        };
    let first = import(&mut h);
    assert!(first.starts_with("Imported 2 songs"), "{first}");
    let second = import(&mut h);
    assert!(
        second.starts_with("Imported 0 songs. 2 already in the library"),
        "{second}"
    );
    // The two seed hymns plus the two from the backup.
    assert_eq!(search(&mut h, "").len(), 4);
}

#[test]
fn an_opensong_data_folder_imports_its_songs_folder() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    // OpenSong keeps Songs beside Sets and Backgrounds.
    let opensong = dir.path().join("OpenSong");
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/lampstand-import/tests/fixtures/opensong");
    std::fs::create_dir_all(opensong.join("Songs/Hymns")).unwrap();
    std::fs::create_dir_all(opensong.join("Sets")).unwrap();
    std::fs::write(opensong.join("Sets/Sunday"), "<set/>").unwrap();
    for (from, to) in [
        ("Amazing Grace", "Songs/Amazing Grace"),
        ("Hymns/Holy Holy Holy", "Songs/Hymns/Holy Holy Holy"),
    ] {
        std::fs::copy(fixtures.join(from), opensong.join(to)).unwrap();
    }
    let message = match ok(h.app.handle(Request::Library(LibraryRequest::ImportFiles {
        paths: vec![opensong.display().to_string()],
    }))) {
        Response::Text(t) => t,
        other => panic!("{other:?}"),
    };
    assert_eq!(message, "Imported 2 songs");
}

/// A real worshipleaderapp.com database, when one is at hand:
/// LAMPSTAND_OSB=path/to/en.osb cargo test -p lampstand-app --test handler -- --ignored
#[test]
#[ignore = "needs LAMPSTAND_OSB"]
fn a_real_song_database_imports() {
    let Some(osb) = std::env::var_os("LAMPSTAND_OSB") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let started = std::time::Instant::now();
    let message = h.app.handle(Request::Library(LibraryRequest::ImportFiles {
        paths: vec![osb.to_string_lossy().into_owned()],
    }));
    eprintln!("{message:?} in {:?}", started.elapsed());
    let started = std::time::Instant::now();
    let hits = search(&mut h, "amazing grace");
    eprintln!("search: {:?} in {:?}", hits.first(), started.elapsed());
    assert!(!hits.is_empty());
}
