//! The request handlers end to end, without Tauri or a GPU: a recording
//! `OutputSink` stands in for the render thread and a fake `WindowHost`
//! for the output windows.

use std::path::Path;
use std::sync::{Arc, Mutex};

use lampstand_api::{
    DeviceSettings, Event, LibraryRequest, LoadedService, OutputRequest, Request, Response,
    ServiceRequest, SettingsRequest,
};
use lampstand_app::handler::{App, Placement, WindowHost};
use lampstand_core::id::Timestamp;
use lampstand_core::output::{
    BackgroundMode, DisplayMatch, OutputConfig, OutputId, OutputRole, OutputTarget,
};
use lampstand_core::program::{Content, Overlay};
use lampstand_core::safe_area::SafeArea;
use lampstand_core::scene::{Direction, Effect, Node, NodeKind, Scene, Transition, TransitionKind};
use lampstand_core::scripture::Translation;
use lampstand_core::service::{CountdownTarget, ItemId, ItemKind, Service, ServiceId};
use lampstand_core::song::SongId;
use lampstand_layout::Fonts;
use lampstand_library::bundled::BUNDLED;
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
    create_service_at(h, title, None)
}

/// A start time in a year that is safely in the future (or the past, for
/// a negative `days`), whatever day the tests run on.
fn days_from_2100(days: i64) -> Timestamp {
    Timestamp(4_102_444_800_000 + days * 24 * 3_600_000)
}

fn create_service_at(h: &mut Harness, title: &str, starts_at: Option<Timestamp>) -> Service {
    match ok(h.app.handle(Request::Service(ServiceRequest::Create {
        title: title.into(),
        starts_at,
        time_zone: starts_at.map(|_| "Europe/London".into()),
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
fn a_fresh_library_has_the_bundled_bibles_once() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let translations = list_translations(&mut h);
    let abbreviations: Vec<&str> = translations
        .iter()
        .map(|t| t.abbreviation.as_str())
        .collect();
    assert_eq!(abbreviations, ["KJV", "WEB"]);
    for t in &translations {
        let passage = h
            .app
            .handle(Request::Library(LibraryRequest::LookupPassage {
                reference: "John 3:16".into(),
                translation: t.id,
            }));
        assert!(
            matches!(passage, Response::Passage { ref verses, .. }
                if verses.len() == 1 && verses[0].2.starts_with("For God so loved the world")),
            "{}: {passage:?}",
            t.abbreviation
        );
    }

    // One the operator deletes stays deleted.
    h.app
        .library()
        .delete_translation(translations[0].id)
        .unwrap();
    drop(h);
    let mut h = open(dir.path());
    assert_eq!(list_translations(&mut h).len(), 1);
}

fn list_translations(h: &mut Harness) -> Vec<Translation> {
    match ok(h
        .app
        .handle(Request::Library(LibraryRequest::ListTranslations)))
    {
        Response::Translations(t) => t,
        other => panic!("{other:?}"),
    }
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
    // The bundled Bibles, then the imported one.
    assert_eq!(translations.len(), BUNDLED.len() + 1);
    let imported = translations
        .iter()
        .find(|t| BUNDLED.iter().all(|b| b.id != t.id))
        .unwrap();
    // The Bible is usable straight away.
    let passage = h
        .app
        .handle(Request::Library(LibraryRequest::LookupPassage {
            reference: "Gen 1:1".into(),
            translation: imported.id,
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
    assert_eq!(listed[0].items, 1, "the song counts");

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

fn list_titles(h: &mut Harness) -> Vec<String> {
    match ok(h.app.handle(Request::Service(ServiceRequest::List))) {
        Response::Services(s) => s.into_iter().map(|s| s.title).collect(),
        other => panic!("{other:?}"),
    }
}

fn reschedule(
    h: &mut Harness,
    service: &Service,
    starts_at: Option<Timestamp>,
    zone: Option<&str>,
) -> Service {
    match ok(h.app.handle(Request::Service(ServiceRequest::Reschedule {
        service: service.id,
        starts_at,
        time_zone: zone.map(Into::into),
    }))) {
        Response::Service(s) => s,
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_service_is_created_at_a_time_and_rescheduled() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let sunday = days_from_2100(2);

    let service = create_service_at(&mut h, "Morning", Some(sunday));
    assert_eq!(service.starts_at, Some(sunday));
    assert_eq!(service.time_zone.as_deref(), Some("Europe/London"));

    let later = days_from_2100(9);
    let moved = reschedule(&mut h, &service, Some(later), Some("America/Jamaica"));
    assert_eq!(moved.starts_at, Some(later));
    match ok(h
        .app
        .handle(Request::Service(ServiceRequest::Get(service.id))))
    {
        Response::Service(s) => {
            assert_eq!(s.starts_at, Some(later));
            assert_eq!(s.time_zone.as_deref(), Some("America/Jamaica"));
        }
        other => panic!("{other:?}"),
    }
    match ok(h.app.handle(Request::Service(ServiceRequest::List))) {
        Response::Services(list) => {
            assert_eq!(list[0].starts_at, Some(later));
            assert_eq!(list[0].time_zone.as_deref(), Some("America/Jamaica"));
            assert_eq!(list[0].items, 0, "nothing planned yet");
        }
        other => panic!("{other:?}"),
    }

    let cleared = reschedule(&mut h, &service, None, None);
    assert_eq!(cleared.starts_at, None);
    assert_eq!(cleared.time_zone, None);
}

#[test]
fn rescheduling_an_unknown_service_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let response = h.app.handle(Request::Service(ServiceRequest::Reschedule {
        service: ServiceId::generate(),
        starts_at: None,
        time_zone: None,
    }));
    assert!(matches!(response, Response::Error { .. }), "{response:?}");
}

#[test]
fn services_list_upcoming_then_undated_then_past() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let past = Timestamp(946_684_800_000); // 2000-01-01
    create_service_at(&mut h, "Second Sunday", Some(days_from_2100(7)));
    create_service_at(&mut h, "Long ago", Some(past));
    create_service(&mut h, "Old draft");
    create_service_at(&mut h, "First Sunday", Some(days_from_2100(0)));
    create_service_at(
        &mut h,
        "Last year",
        Some(Timestamp(past.0 + 365 * 24 * 3_600_000)),
    );
    // IDs order by the millisecond they were made in.
    std::thread::sleep(std::time::Duration::from_millis(2));
    create_service(&mut h, "New draft");

    assert_eq!(
        list_titles(&mut h),
        [
            "First Sunday",
            "Second Sunday",
            "New draft",
            "Old draft",
            "Last year",
            "Long ago"
        ]
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
fn the_ui_is_told_when_displays_change_so_it_need_not_be_refreshed() {
    let dir = tempfile::tempdir().unwrap();
    let host = Host::default();
    let mut h = open_with(dir.path(), host.clone());
    let told = |h: &Harness| {
        h.events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| matches!(e, Event::DisplaysChanged))
            .count()
    };
    assert_eq!(told(&h), 0);
    host.0.lock().unwrap().displays.push(DisplayInfo {
        name: "Projector".into(),
        width: 1920,
        height: 1080,
        x: 1512,
        y: 0,
    });
    h.app.displays_changed();
    assert_eq!(told(&h), 1);
    host.0.lock().unwrap().displays.clear();
    h.app.displays_changed();
    assert_eq!(told(&h), 2, "unplugging one is announced too");
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
fn deleting_every_theme_mid_service_falls_back_to_the_starter_theme() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let output = window_output();
    ok(h.app
        .handle(Request::Outputs(OutputRequest::Put(output.clone()))));
    let service = create_service(&mut h, "Sunday");
    let (song, _) = search(&mut h, "amazing grace")[0].clone();
    let service = insert_song(&mut h, &service, song);
    open_service(&mut h, &service);
    ok(h.app.handle(Request::Live(Command::Next)));

    // Another device deletes the only theme; the delete syncs in.
    let library = h.app.library().clone();
    let themes = library.themes().unwrap();
    for theme in &themes {
        library.delete_theme(theme.id).unwrap();
    }
    assert!(library.themes().unwrap().is_empty());
    let changes: Vec<_> = themes
        .iter()
        .map(|t| lampstand_sync::DocChanged {
            kind: lampstand_sync::DocKind::Theme,
            id: t.id.uuid(),
            remote: true,
        })
        .collect();
    h.app.library_changed(&changes);
    h.sink.take();

    ok(h.app.handle(Request::Live(Command::Next)));
    let shown = shown_scenes(&h.sink);
    assert_eq!(shown.len(), 1);
    assert!(
        scene_text(&shown[0].1).contains("grace that taught my heart"),
        "{}",
        scene_text(&shown[0].1)
    );
}

#[test]
fn unbuilt_areas_answer_with_an_error_not_a_panic() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    for request in [
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

#[test]
fn a_countdown_recomposes_each_second_and_only_then() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    ok(h.app
        .handle(Request::Outputs(OutputRequest::Put(window_output()))));
    let service = create_service(&mut h, "Sunday");
    let t0 = Timestamp(1_800_000_000_000);
    ok(h.app.handle(Request::Service(ServiceRequest::InsertItem {
        service: service.id,
        index: usize::MAX,
        kind: ItemKind::Countdown {
            target: CountdownTarget::At(Timestamp(t0.0 + 10_000)),
            label: "Starts".into(),
        },
    })));
    open_service(&mut h, &service);
    ok(h.app.handle(Request::Live(Command::Next)));

    // The first tick starts the engine's clock and shows the real time left.
    h.app.tick_at(t0);
    h.sink.take();
    h.events.lock().unwrap().clear();

    // Ticks within the same displayed second send nothing.
    h.app.tick_at(Timestamp(t0.0 + 100));
    h.app.tick_at(Timestamp(t0.0 + 900));
    assert!(h.sink.take().is_empty());

    // The next second sends a new scene to the output, showing it.
    h.app.tick_at(Timestamp(t0.0 + 1000));
    let shown = shown_scenes(&h.sink);
    assert_eq!(shown.len(), 1);
    assert!(scene_text(&shown[0].1).contains("0:09"));
    h.app.tick_at(Timestamp(t0.0 + 2000));
    let shown = shown_scenes(&h.sink);
    assert_eq!(shown.len(), 1);
    assert!(scene_text(&shown[0].1).contains("0:08"));

    // Nothing about the live state changed, so the UI was not told.
    assert!(!h
        .events
        .lock()
        .unwrap()
        .iter()
        .any(|e| matches!(e, Event::Live { .. })));

    // Zero is drawn once; after that the output holds still.
    h.app.tick_at(Timestamp(t0.0 + 10_000));
    assert!(scene_text(&shown_scenes(&h.sink)[0].1).contains("0:00"));
    h.app.tick_at(Timestamp(t0.0 + 11_000));
    assert!(h.sink.take().is_empty());
}

fn deck_path(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/lampstand-slides/tests/decks")
        .join(format!("{name}.pptx"))
        .display()
        .to_string()
}

fn insert(h: &mut Harness, service: &Service, kind: ItemKind) -> Service {
    match ok(h.app.handle(Request::Service(ServiceRequest::InsertItem {
        service: service.id,
        index: usize::MAX,
        kind,
    }))) {
        Response::Service(s) => s,
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_deck_and_a_video_from_disk_play_in_a_service() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let output = window_output();
    ok(h.app
        .handle(Request::Outputs(OutputRequest::Put(output.clone()))));

    let deck = match ok(h.app.handle(Request::Library(LibraryRequest::AddDeckFile {
        path: deck_path("10-church-service"),
    }))) {
        Response::Deck(d) => d,
        other => panic!("{other:?}"),
    };
    assert_eq!(deck.title, "10-church-service");
    let listed = match ok(h.app.handle(Request::Library(LibraryRequest::ListDecks))) {
        Response::Decks(d) => d,
        other => panic!("{other:?}"),
    };
    assert_eq!(listed, vec![deck.clone()]);

    let video = dir.path().join("Welcome loop.mp4");
    std::fs::write(&video, b"not really a video").unwrap();
    let media = match ok(h.app.handle(Request::Library(LibraryRequest::AddMediaFile {
        path: video.display().to_string(),
    }))) {
        Response::Media(m) => m,
        other => panic!("{other:?}"),
    };
    assert_eq!(media.name, "Welcome loop.mp4");

    let service = create_service(&mut h, "Sunday");
    let service = insert(&mut h, &service, ItemKind::Deck { deck: deck.id });
    let service = insert(
        &mut h,
        &service,
        ItemKind::Media {
            blob: media.blob,
            name: media.name.clone(),
            looped: true,
        },
    );
    let listed = match ok(h.app.handle(Request::Library(LibraryRequest::ListMedia))) {
        Response::MediaFiles(m) => m,
        other => panic!("{other:?}"),
    };
    assert_eq!(listed, vec![media.clone()]);

    let loaded = open_service(&mut h, &service);
    assert_eq!(loaded.items[0].title, "10-church-service");
    // The deck's hidden slide is left out.
    let slides = &loaded.items[0].slides;
    assert!(slides.len() >= 2, "{slides:?}");
    assert_eq!(slides[0].label.as_deref(), Some("Slide 1"));
    assert_eq!(loaded.items[1].title, "Welcome loop.mp4");
    assert_eq!(loaded.items[1].slides[0].label.as_deref(), Some("Video"));
    // Deck slides have a scene for the operator's thumbnails; video has none.
    assert!(h.app.slide_scene(loaded.items[0].id, 0, 0).is_some());
    assert!(h.app.slide_scene(loaded.items[1].id, 0, 0).is_none());
    h.sink.take();

    ok(h.app.handle(Request::Live(Command::Next)));
    let program = h.app.program().clone();
    assert!(
        matches!(&program.content, Some(Content::DeckSlide { deck: d, index: 0, .. }) if *d == deck.id),
        "{program:?}"
    );
    let shown = shown_scenes(&h.sink);
    assert_eq!(shown.len(), 1);
    assert!(scene_text(&shown[0].1).contains("Amazing grace, how sweet the sound"));

    for _ in 0..slides.len() {
        ok(h.app.handle(Request::Live(Command::Next)));
    }
    let program = h.app.program().clone();
    assert!(
        matches!(&program.content, Some(Content::Video { blob, looped: true }) if *blob == media.blob),
        "{program:?}"
    );
}

#[test]
fn a_file_that_is_not_a_deck_is_refused_and_a_missing_deck_still_loads() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let path = dir.path().join("hymns.pptx");
    std::fs::write(&path, b"plain text").unwrap();
    let response = h.app.handle(Request::Library(LibraryRequest::AddDeckFile {
        path: path.display().to_string(),
    }));
    assert!(
        matches!(&response, Response::Error { message } if message.contains("not a PowerPoint")),
        "{response:?}"
    );

    let service = create_service(&mut h, "Sunday");
    let service = insert(
        &mut h,
        &service,
        ItemKind::Deck {
            deck: lampstand_core::deck::DeckId::generate(),
        },
    );
    let loaded = open_service(&mut h, &service);
    assert_eq!(loaded.items[0].title, "Missing presentation");
    assert!(loaded.items[0].slides.is_empty());
}

/// Whether the node with this ID is drawn: it and every group around it
/// visible. `None` when the scene has no such node.
fn drawn(nodes: &[Node], id: u32) -> Option<bool> {
    nodes.iter().find_map(|n| {
        if n.id == id {
            return Some(n.visible);
        }
        match &n.kind {
            NodeKind::Group(kids) => drawn(kids, id).map(|v| v && n.visible),
            _ => None,
        }
    })
}

#[test]
fn deck_builds_animate_on_the_output_and_survive_a_recompose() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let output = window_output();
    ok(h.app
        .handle(Request::Outputs(OutputRequest::Put(output.clone()))));
    let deck = match ok(h.app.handle(Request::Library(LibraryRequest::AddDeckFile {
        path: deck_path("11-builds-transitions"),
    }))) {
        Response::Deck(d) => d,
        other => panic!("{other:?}"),
    };
    let service = create_service(&mut h, "Sunday");
    let service = insert(&mut h, &service, ItemKind::Deck { deck: deck.id });
    let loaded = open_service(&mut h, &service);
    let item = loaded.items[0].id;
    let slides = &loaded.items[0].slides;
    let steps: Vec<usize> = slides.iter().map(|s| s.steps).collect();
    assert_eq!(steps, [0, 3, 2, 2, 0]);
    assert_eq!(
        slides[1].notes,
        "Reveal one point at a time.\nPause after the second."
    );
    assert_eq!(slides[0].notes, "");
    h.sink.take();

    // Each slide arrives with its own transition, not the theme's.
    let shows = |h: &Harness| -> Vec<(Arc<Scene>, Transition)> {
        h.sink
            .take()
            .into_iter()
            .filter_map(|(_, c)| match c {
                OutputCommand::Show { scene, transition } => Some((scene, transition)),
                _ => None,
            })
            .collect()
    };
    ok(h.app.handle(Request::Live(Command::Next)));
    let shown = shows(&h);
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].1.kind, TransitionKind::Fade);
    ok(h.app.handle(Request::Live(Command::Next)));
    let shown = shows(&h);
    assert_eq!(shown[0].1.kind, TransitionKind::Push(Direction::Down));
    // The three paragraphs' nodes, in build order.
    let paragraphs: Vec<u32> = (0..3)
        .map(|step| {
            let before = h.app.slide_scene(item, 1, step).unwrap();
            let after = h.app.slide_scene(item, 1, step + 1).unwrap();
            find_revealed(&before.nodes, &after.nodes).unwrap_or_else(|| panic!("step {step}"))
        })
        .collect();
    assert!(paragraphs
        .iter()
        .all(|id| drawn(&shown[0].0.nodes, *id) == Some(false)));

    // A click plays the build: `Animate`, never a `Show` that would cut
    // the animation short.
    ok(h.app.handle(Request::Live(Command::Next)));
    let sent = h.sink.take();
    assert_eq!(sent.len(), 1, "{sent:?}");
    let OutputCommand::Animate(animations) = &sent[0].1 else {
        panic!("{sent:?}")
    };
    assert_eq!(animations.len(), 1);
    assert_eq!(animations[0].effect, Effect::FadeIn);
    assert_eq!(animations[0].node, paragraphs[0]);

    // A recompose (Black on and off) keeps what has built.
    ok(h.app.handle(Request::Live(Command::ToggleBlack)));
    h.sink.take();
    ok(h.app.handle(Request::Live(Command::ToggleBlack)));
    let shown = shows(&h);
    let visible: Vec<Option<bool>> = paragraphs
        .iter()
        .map(|id| drawn(&shown[0].0.nodes, *id))
        .collect();
    assert_eq!(visible, [Some(true), Some(false), Some(false)]);

    // Back through a build cuts to the step before.
    ok(h.app.handle(Request::Live(Command::Previous)));
    let shown = shows(&h);
    assert_eq!(shown[0].1, Transition::CUT);
    assert_eq!(drawn(&shown[0].0.nodes, paragraphs[0]), Some(false));

    // Thumbnails show a slide at any step.
    let thumb = h.app.slide_scene(item, 1, 2).unwrap();
    let visible: Vec<Option<bool>> = paragraphs
        .iter()
        .map(|id| drawn(&thumb.nodes, *id))
        .collect();
    assert_eq!(visible, [Some(true), Some(true), Some(false)]);

    // A slide whose first effect plays by itself arrives without it and
    // then animates it.
    ok(h.app
        .handle(Request::Live(Command::Show(lampstand_live::Cursor {
            item,
            slide: 3,
            step: 0,
        }))));
    let sent = h.sink.take();
    let kinds: Vec<&str> = sent
        .iter()
        .map(|(_, c)| match c {
            OutputCommand::Show { .. } => "show",
            OutputCommand::Animate(_) => "animate",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds, ["show", "animate"]);
    let (OutputCommand::Show { scene, transition }, OutputCommand::Animate(entry)) =
        (&sent[0].1, &sent[1].1)
    else {
        unreachable!()
    };
    assert_eq!(transition.kind, TransitionKind::Cover(Direction::Right));
    assert_eq!(entry.len(), 1);
    assert_eq!(drawn(&scene.nodes, entry[0].node), Some(false));
    // Composed again, it stays shown.
    let thumb = h.app.slide_scene(item, 3, 0).unwrap();
    assert_eq!(drawn(&thumb.nodes, entry[0].node), Some(true));
}

/// The node hidden in `before` and drawn in `after`.
fn find_revealed(before: &[Node], after: &[Node]) -> Option<u32> {
    fn ids(nodes: &[Node], out: &mut Vec<u32>) {
        for n in nodes {
            out.push(n.id);
            if let NodeKind::Group(kids) = &n.kind {
                ids(kids, out);
            }
        }
    }
    let mut all = Vec::new();
    ids(before, &mut all);
    all.into_iter()
        .find(|id| drawn(before, *id) == Some(false) && drawn(after, *id) == Some(true))
}

fn set_obs_scene(h: &mut Harness, service: &Service, item: ItemId, scene: Option<&str>) -> Service {
    match ok(h.app.handle(Request::Service(ServiceRequest::SetObsScene {
        service: service.id,
        item,
        scene: scene.map(str::to_string),
    }))) {
        Response::Service(s) => s,
        other => panic!("{other:?}"),
    }
}

/// T90: each item going live, forwards or back, cues its OBS scene;
/// moving within an item doesn't.
#[test]
fn items_going_live_cue_their_obs_scenes() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let (cues, mut received) = tokio::sync::mpsc::unbounded_channel();
    h.app.set_obs_cues(cues);
    let service = create_service(&mut h, "Sunday");
    let countdown = |label: &str| ItemKind::Countdown {
        target: CountdownTarget::At(Timestamp(1_800_000_000_000)),
        label: label.into(),
    };
    insert(&mut h, &service, countdown("Welcome"));
    let s = insert(&mut h, &service, countdown("Sermon"));
    let order: Vec<ItemId> = s.ordered().into_iter().map(|(id, _)| id).collect();

    let s = set_obs_scene(&mut h, &service, order[0], Some("  Worship  "));
    assert_eq!(s.items[&order[0]].obs_scene.as_deref(), Some("Worship"));
    let s = set_obs_scene(&mut h, &service, order[1], Some("   "));
    assert_eq!(s.items[&order[1]].obs_scene, None, "blank means none");
    set_obs_scene(&mut h, &service, order[1], Some("Pastor and slides"));
    open_service(&mut h, &service);
    assert!(received.try_recv().is_err(), "opening shows nothing yet");

    let mut next = |h: &mut Harness, command: Command| {
        ok(h.app.handle(Request::Live(command)));
        let mut got = Vec::new();
        while let Ok(cue) = received.try_recv() {
            got.push((cue.item, cue.scene));
        }
        got
    };
    assert_eq!(
        next(&mut h, Command::Next),
        [(order[0], Some("Worship".to_string()))]
    );
    assert_eq!(
        next(&mut h, Command::Next),
        [(order[1], Some("Pastor and slides".to_string()))]
    );
    assert_eq!(
        next(&mut h, Command::Previous),
        [(order[0], Some("Worship".to_string()))]
    );
    // Black keeps the item live: no cue.
    assert!(next(&mut h, Command::ToggleBlack).is_empty());

    // Clearing a scene: that item cues `None` (it still ends a hold).
    set_obs_scene(&mut h, &service, order[1], None);
    assert_eq!(next(&mut h, Command::Next), [(order[1], None)]);
}

#[test]
fn setting_the_scene_of_an_unknown_item_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let service = create_service(&mut h, "Sunday");
    let response = h.app.handle(Request::Service(ServiceRequest::SetObsScene {
        service: service.id,
        item: ItemId::generate(),
        scene: Some("Worship".into()),
    }));
    assert!(matches!(response, Response::Error { .. }), "{response:?}");
}

#[test]
fn clicker_mode_is_a_device_setting_and_takes_keys_only_once_a_service_is_open() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let settings = |h: &mut Harness, request| match ok(h.app.handle(Request::Settings(request))) {
        Response::Settings(s) => s,
        other => panic!("{other:?}"),
    };
    assert!(!settings(&mut h, SettingsRequest::Get).clicker_mode);
    let on = DeviceSettings { clicker_mode: true };
    assert_eq!(settings(&mut h, SettingsRequest::Put(on.clone())), on);
    let state = h.app.key_state();
    assert!(state.clicker_mode && !state.live);
    assert!(state.wanted().is_empty(), "nothing open, nothing taken");

    let (song, _) = search(&mut h, "Amazing Grace")[0].clone();
    let service = create_service(&mut h, "Sunday");
    insert_song(&mut h, &service, song);
    open_service(&mut h, &service);
    assert_eq!(
        h.app.key_state().wanted(),
        lampstand_app::keys::Key::CLICKER.into_iter().collect()
    );

    // It stays on this machine across restarts.
    drop(h);
    let h = open(dir.path());
    assert!(h.app.key_state().clicker_mode);
}
