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
use lampstand_core::scripture::{Translation, TranslationId};
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
        idle: None,
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

/// Looks `reference` up in `translation`, the default for a reference that
/// names no version.
fn lookup(h: &mut Harness, reference: &str, translation: TranslationId) -> Response {
    h.app
        .handle(Request::Library(LibraryRequest::LookupPassage {
            reference: reference.into(),
            translation,
        }))
}

#[test]
fn a_version_typed_after_the_reference_picks_the_translation() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let translations = list_translations(&mut h);
    let id = |abbreviation: &str| {
        translations
            .iter()
            .find(|t| t.abbreviation == abbreviation)
            .unwrap()
            .id
    };
    let (kjv, web) = (id("KJV"), id("WEB"));
    let text = |response: Response| match response {
        Response::Passage {
            translation,
            display,
            verses,
            ..
        } => (translation, display, verses[0].2.clone()),
        other => panic!("{other:?}"),
    };

    // No version typed: the default asked for.
    let (used, display, plain) = text(lookup(&mut h, "James 3:11", web));
    assert_eq!((used, display.as_str()), (web, "James 3:11"));
    // A version typed wins, however it is cased or bracketed.
    for typed in [
        "James 3:11 kjv",
        "james 3:11 KJV",
        "James 3:11 (King James Version)",
    ] {
        let (used, _, verse) = text(lookup(&mut h, typed, web));
        assert_eq!(used, kjv, "{typed}");
        assert!(verse.contains("fountain"), "{typed}: {verse}");
    }
    assert!(plain.contains("spring"), "{plain}");
    let (used, ..) = text(lookup(&mut h, "James 3:11 web", kjv));
    assert_eq!(used, web);
}

#[test]
fn an_unrecognised_version_lists_the_ones_there_are() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let web = list_translations(&mut h)
        .iter()
        .find(|t| t.abbreviation == "WEB")
        .unwrap()
        .id;
    // The ESV is licensed, so it is not bundled: until a church imports it,
    // it is a version that is not there.
    for typed in ["James 3:11 xyz", "James 3:11 ESV"] {
        match lookup(&mut h, typed, web) {
            Response::Error { message } => {
                let version = typed.rsplit(' ').next().unwrap();
                assert_eq!(
                    message,
                    format!("Unrecognised version “{version}”. Available versions: KJV, WEB.")
                );
            }
            other => panic!("{typed}: {other:?}"),
        }
    }
}

#[test]
fn the_default_version_is_a_device_setting_that_stays_across_restarts() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let settings = |h: &mut Harness, request| match ok(h.app.handle(Request::Settings(request))) {
        Response::Settings(s) => s,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        settings(&mut h, SettingsRequest::Get).default_translation,
        None
    );
    let kjv = list_translations(&mut h)
        .iter()
        .find(|t| t.abbreviation == "KJV")
        .unwrap()
        .id;
    let chosen = DeviceSettings {
        default_translation: Some(kjv),
        ..DeviceSettings::default()
    };
    assert_eq!(
        settings(&mut h, SettingsRequest::Put(chosen.clone())),
        chosen
    );
    drop(h);
    let mut h = open(dir.path());
    assert_eq!(settings(&mut h, SettingsRequest::Get), chosen);
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
    // The first slide credits the author as well as the copyright.
    assert_eq!(slide.footer.as_deref(), Some("John Newton | Public Domain"));

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
fn an_update_waits_while_something_is_on_air_or_a_service_is_about_to_start() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let starts = days_from_2100(0);
    let hour = 60 * 60 * 1000;
    let service = create_service_at(&mut h, "Sunday", Some(starts));

    // Quiet: nothing on air, and the service is a day away.
    assert_eq!(
        h.app.update_blocker_at(Timestamp(starts.0 - 24 * hour)),
        None
    );
    // Half an hour before it starts, an update would be in the way.
    let soon = h
        .app
        .update_blocker_at(Timestamp(starts.0 - hour / 2))
        .unwrap();
    assert!(soon.contains("starts in 30 minutes"), "{soon}");

    // Once something is on air, it waits however far off the next service is.
    let hits = search(&mut h, "");
    let service = insert_song(&mut h, &service, hits[0].0);
    open_service(&mut h, &service);
    assert_eq!(
        h.app.update_blocker_at(Timestamp(starts.0 - 24 * hour)),
        None,
        "open is not on air"
    );
    ok(h.app.handle(Request::Live(Command::Next)));
    let on_air = h
        .app
        .update_blocker_at(Timestamp(starts.0 - 24 * hour))
        .unwrap();
    assert!(on_air.contains("on air"), "{on_air}");
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

fn set_arrangement(
    h: &mut Harness,
    service: &Service,
    item: ItemId,
    arrangement: Option<Vec<String>>,
) -> Response {
    h.app
        .handle(Request::Service(ServiceRequest::SetItemArrangement {
            service: service.id,
            item,
            arrangement,
        }))
}

/// The label of each slide of the first item of the open service.
fn slide_labels(loaded: &LoadedService) -> Vec<String> {
    loaded.items[0]
        .slides
        .iter()
        .filter_map(|s| s.label.clone())
        .collect()
}

#[test]
fn a_service_items_order_is_set_applied_to_the_open_service_and_reset() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let (song, _) = search(&mut h, "Amazing Grace")[0].clone();
    let empty = create_service(&mut h, "Sunday");
    let service = insert_song(&mut h, &empty, song);
    let item = service.ordered()[0].0;
    let sections = match ok(h
        .app
        .handle(Request::Library(LibraryRequest::GetSong(song))))
    {
        Response::Song(s) => s.sections,
        other => panic!("{other:?}"),
    };
    let (a, b) = (sections[0].label.clone(), sections[1].label.clone());
    let written = slide_labels(&open_service(&mut h, &service));

    // Typed lower case, a repeat, and the rest left out.
    let order = vec![
        a.to_lowercase(),
        a.to_lowercase(),
        b.to_lowercase(),
        a.to_lowercase(),
    ];
    let Response::Service(saved) = ok(set_arrangement(&mut h, &service, item, Some(order))) else {
        panic!()
    };
    let ItemKind::Song { arrangement, .. } = &saved.items[&item].kind else {
        panic!()
    };
    assert!(arrangement.is_some());
    // The open service reloaded with the new order.
    let loaded = open_service(&mut h, &service);
    let got = slide_labels(&loaded);
    let groups: Vec<&str> = {
        let mut g = got.iter().map(String::as_str).collect::<Vec<_>>();
        g.dedup();
        g
    };
    assert!(groups.len() < written.len() || sections.len() <= 2);
    assert_eq!(got.first(), Some(&a));
    assert_eq!(got.last(), Some(&a));

    // Blank goes back to the song's own order.
    ok(set_arrangement(
        &mut h,
        &service,
        item,
        Some(vec![" ".into()]),
    ));
    assert_eq!(slide_labels(&open_service(&mut h, &service)), written);
    ok(set_arrangement(&mut h, &service, item, None));
}

#[test]
fn only_a_song_has_an_order() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let service = create_service(&mut h, "Sunday");
    let s = insert(
        &mut h,
        &service,
        ItemKind::Header {
            title: "Worship".into(),
        },
    );
    let item = s.ordered()[0].0;
    let response = set_arrangement(&mut h, &service, item, Some(vec!["v1".into()]));
    assert!(matches!(response, Response::Error { .. }), "{response:?}");
    let response = set_arrangement(&mut h, &service, ItemId::generate(), None);
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
    let on = DeviceSettings {
        clicker_mode: true,
        ..DeviceSettings::default()
    };
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

#[test]
fn editing_a_song_in_an_open_service_changes_what_is_shown() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    ok(h.app
        .handle(Request::Outputs(OutputRequest::Put(window_output()))));
    let service = create_service(&mut h, "Sunday");
    let (id, _) = search(&mut h, "amazing grace")[0].clone();
    let service = insert_song(&mut h, &service, id);
    open_service(&mut h, &service);

    // The editor loads the song, changes a line and puts it back.
    let Response::Song(mut song) = ok(h.app.handle(Request::Library(LibraryRequest::GetSong(id))))
    else {
        panic!("expected a song");
    };
    let first = &mut song.sections[0].lines[0];
    assert!(!first.contains("[G]"));
    *first = "[G]Edited line [C]here".into();
    ok(h.app
        .handle(Request::Library(LibraryRequest::PutSong(song))));
    h.app.library_changed(&[lampstand_sync::DocChanged {
        kind: lampstand_sync::DocKind::Song,
        id: id.uuid(),
        remote: false,
    }]);
    h.sink.take();

    ok(h.app.handle(Request::Live(Command::Next)));
    let shown = shown_scenes(&h.sink);
    assert_eq!(shown.len(), 1);
    assert!(
        scene_text(&shown[0].1).contains("Edited line here"),
        "{}",
        scene_text(&shown[0].1)
    );
}

fn service_request(h: &mut Harness, request: ServiceRequest) -> Response {
    h.app.handle(Request::Service(request))
}

fn header(title: &str) -> ItemKind {
    ItemKind::Header {
        title: title.into(),
    }
}

#[test]
fn a_service_is_renamed_closed_and_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let service = create_service(&mut h, "Sunday");
    insert(&mut h, &service, header("Welcome"));
    open_service(&mut h, &service);

    let renamed = match ok(service_request(
        &mut h,
        ServiceRequest::Rename {
            service: service.id,
            title: "  Easter  ".into(),
        },
    )) {
        Response::Service(s) => s,
        other => panic!("{other:?}"),
    };
    assert_eq!(renamed.title, "Easter");
    assert!(matches!(
        service_request(
            &mut h,
            ServiceRequest::Rename {
                service: service.id,
                title: "   ".into()
            }
        ),
        Response::Error { .. }
    ));

    ok(service_request(&mut h, ServiceRequest::Close(service.id)));
    assert_eq!(list_titles(&mut h), ["Easter"], "off air keeps the service");

    open_service(&mut h, &service);
    ok(service_request(&mut h, ServiceRequest::Delete(service.id)));
    assert!(list_titles(&mut h).is_empty());
}

#[test]
fn a_section_loop_is_changed_on_air_and_the_saved_loop_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let service = create_service(&mut h, "Sunday");
    insert(&mut h, &service, header("Pre-service"));
    let s = insert(
        &mut h,
        &service,
        ItemKind::Countdown {
            target: CountdownTarget::At(Timestamp(1_800_000_000_000)),
            label: "Starts".into(),
        },
    );
    let order: Vec<ItemId> = s.ordered().into_iter().map(|(id, _)| id).collect();
    ok(service_request(
        &mut h,
        ServiceRequest::SetLoop {
            service: service.id,
            item: order[0],
            seconds: Some(8),
        },
    ));

    let loaded = open_service(&mut h, &service);
    assert_eq!(loaded.sections.len(), 1);
    assert_eq!(loaded.sections[0].seconds, Some(8));
    assert_eq!(loaded.sections[0].items, [order[1]]);

    ok(service_request(
        &mut h,
        ServiceRequest::SetLiveLoop {
            header: order[0],
            seconds: Some(12),
        },
    ));
    let announced = h
        .events
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find_map(|e| match e {
            Event::Loaded(l) => Some(l.clone()),
            _ => None,
        })
        .expect("the operator view is told");
    assert_eq!(announced.sections[0].seconds, Some(12));

    ok(service_request(
        &mut h,
        ServiceRequest::SetLiveLoop {
            header: order[0],
            seconds: None,
        },
    ));
    let saved = match ok(service_request(&mut h, ServiceRequest::Get(service.id))) {
        Response::Service(s) => s,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        saved.items[&order[0]].loop_seconds,
        Some(8),
        "the saved service is not changed by what happens on air"
    );

    // Another service forgets it.
    let other = create_service(&mut h, "Evening");
    open_service(&mut h, &other);
    let again = open_service(&mut h, &service);
    assert_eq!(again.sections[0].seconds, Some(8));
}

#[test]
fn the_estimate_counts_slides_and_flags_what_has_no_fixed_length() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let service = create_service(&mut h, "Sunday");
    insert(&mut h, &service, header("Pre-service"));
    let s = insert(
        &mut h,
        &service,
        ItemKind::Countdown {
            target: CountdownTarget::At(Timestamp(1_800_000_000_000)),
            label: "Starts".into(),
        },
    );
    let order: Vec<ItemId> = s.ordered().into_iter().map(|(id, _)| id).collect();
    let estimates = match ok(service_request(
        &mut h,
        ServiceRequest::Estimate(service.id),
    )) {
        Response::Estimate(list) => list,
        other => panic!("{other:?}"),
    };
    assert_eq!(estimates.len(), 1, "headers have nothing on screen");
    assert_eq!(estimates[0].item, order[1]);
    assert!(estimates[0].open_ended);
    assert_eq!(estimates[0].stills, 0);
}

fn put_theme(h: &mut Harness, theme: lampstand_core::theme::Theme) {
    ok(h.app
        .handle(Request::Library(LibraryRequest::PutTheme(theme))));
}

fn list_themes(h: &mut Harness) -> Vec<lampstand_core::theme::Theme> {
    match ok(h.app.handle(Request::Library(LibraryRequest::ListThemes))) {
        Response::Themes(t) => t,
        other => panic!("{other:?}"),
    }
}

fn error_message(response: Response) -> String {
    match response {
        Response::Error { message } => message,
        other => panic!("expected an error, got {other:?}"),
    }
}

fn bg(theme: &lampstand_core::theme::Theme) -> lampstand_core::geom::Color {
    match theme.background {
        lampstand_core::theme::Background::Color(c) => c,
        _ => unreachable!(),
    }
}

fn colored_theme(name: &str, color: lampstand_core::geom::Color) -> lampstand_core::theme::Theme {
    let mut theme = lampstand_library::starter_theme();
    theme.id = lampstand_core::theme::ThemeId::generate();
    theme.name = name.into();
    theme.background = lampstand_core::theme::Background::Color(color);
    theme
}

/// The colour of the first solid fill after the black base: the theme's
/// background.
fn background_color(scene: &Scene) -> lampstand_core::geom::Color {
    use lampstand_core::scene::Paint;
    scene
        .nodes
        .iter()
        .skip(1)
        .find_map(|n| match &n.kind {
            NodeKind::Path(p) => match &p.fill {
                Some(Paint::Solid(c)) => Some(*c),
                _ => None,
            },
            _ => None,
        })
        .expect("a background fill")
}

#[test]
fn themes_are_listed_saved_and_deleted_but_never_the_starter_or_the_last() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let starter = list_themes(&mut h);
    assert_eq!(starter.len(), 1);
    let mine = colored_theme("Mine", lampstand_core::geom::Color::rgb(0, 0, 90));
    put_theme(&mut h, mine.clone());
    assert_eq!(list_themes(&mut h).len(), 2);

    let delete = |h: &mut Harness, id| {
        h.app
            .handle(Request::Library(LibraryRequest::DeleteTheme(id)))
    };
    let message = error_message(delete(&mut h, starter[0].id));
    assert!(message.contains("Starter"), "{message}");
    ok(delete(&mut h, mine.id));
    assert_eq!(list_themes(&mut h), starter);
    // A theme that is already gone, with only the starter left.
    assert!(matches!(delete(&mut h, mine.id), Response::Error { .. }));

    // Editing the starter and restoring it.
    let mut edited = starter[0].clone();
    edited.text.size = 12.0;
    put_theme(&mut h, edited);
    ok(h.app
        .handle(Request::Library(LibraryRequest::RestoreStarterTheme)));
    assert_eq!(list_themes(&mut h), starter);
}

#[test]
fn a_screens_theme_beats_the_services_which_beats_the_default() {
    use lampstand_core::geom::Color;
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let (red, green, blue) = (
        colored_theme("Red", Color::rgb(200, 0, 0)),
        colored_theme("Green", Color::rgb(0, 200, 0)),
        colored_theme("Blue", Color::rgb(0, 0, 200)),
    );
    for t in [&red, &green, &blue] {
        put_theme(&mut h, t.clone());
    }
    let mut output = window_output();
    ok(h.app
        .handle(Request::Outputs(OutputRequest::Put(output.clone()))));
    let mut service = create_service(&mut h, "Sunday");
    let (song, _) = search(&mut h, "amazing grace")[0].clone();
    service = insert_song(&mut h, &service, song);
    open_service(&mut h, &service);
    let on_air = |h: &mut Harness| {
        h.sink.take();
        ok(h.app.handle(Request::Live(Command::Next)));
        let shown = shown_scenes(&h.sink);
        let c = background_color(&shown.last().unwrap().1);
        ok(h.app.handle(Request::Live(Command::ClearAll)));
        c
    };
    // Nothing chosen anywhere: the starter's black.
    assert_eq!(on_air(&mut h), Color::BLACK);

    // The device default.
    let settings = DeviceSettings {
        default_theme: Some(blue.id),
        ..DeviceSettings::default()
    };
    ok(h.app
        .handle(Request::Settings(SettingsRequest::Put(settings))));
    assert_eq!(on_air(&mut h), bg(&blue));

    // The service's own theme beats the default.
    service.theme = Some(green.id);
    let library = h.app.library().clone();
    library.put_service(&service).unwrap();
    h.app.library_changed(&[lampstand_sync::DocChanged {
        kind: lampstand_sync::DocKind::Service,
        id: service.id.uuid(),
        remote: false,
    }]);
    assert_eq!(on_air(&mut h), bg(&green));

    // The screen's beats the service's.
    output.theme = Some(red.id);
    ok(h.app.handle(Request::Outputs(OutputRequest::Put(output))));
    assert_eq!(on_air(&mut h), bg(&red));
}

#[test]
fn songs_are_split_with_the_main_screens_theme() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let mut two_lines = colored_theme("Two lines", lampstand_core::geom::Color::BLACK);
    two_lines.max_lines = 1;
    put_theme(&mut h, two_lines.clone());
    let service = create_service(&mut h, "Sunday");
    let (song, _) = search(&mut h, "amazing grace")[0].clone();
    let service = insert_song(&mut h, &service, song);
    let lines = |loaded: &LoadedService| -> usize {
        loaded.items[0]
            .slides
            .iter()
            .map(|s| s.lines.len())
            .max()
            .unwrap()
    };
    let before = lines(&open_service(&mut h, &service));
    assert!(before > 1);

    let mut output = window_output();
    output.theme = Some(two_lines.id);
    ok(h.app.handle(Request::Outputs(OutputRequest::Put(output))));
    assert_eq!(lines(&open_service(&mut h, &service)), 1);
}

fn blob_with(h: &Harness, bytes: &[u8]) -> lampstand_core::id::BlobHash {
    h.app.library().repo().blobs().put(bytes).unwrap()
}

fn count_kind(scene: &Scene, wanted: &str) -> usize {
    fn walk(nodes: &[Node], wanted: &str) -> usize {
        nodes
            .iter()
            .map(|n| {
                let here = match (&n.kind, wanted) {
                    (NodeKind::Image(_), "image") | (NodeKind::Video(_), "video") => 1,
                    _ => 0,
                };
                here + match &n.kind {
                    NodeKind::Group(c) => walk(c, wanted),
                    _ => 0,
                }
            })
            .sum()
    }
    walk(&scene.nodes, wanted)
}

#[test]
fn idle_shows_until_something_goes_on_air_and_black_and_logo_cover_it() {
    use lampstand_core::output::IdleScreen;
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let image = blob_with(&h, b"a picture");
    let settings = DeviceSettings {
        idle: Some(IdleScreen::Image {
            blob: image,
            name: "Welcome.png".into(),
        }),
        ..DeviceSettings::default()
    };
    ok(h.app
        .handle(Request::Settings(SettingsRequest::Put(settings))));
    let output = window_output();
    ok(h.app
        .handle(Request::Outputs(OutputRequest::Put(output.clone()))));
    let last = |h: &Harness| shown_scenes(&h.sink).pop().expect("a scene was sent").1;
    assert_eq!(count_kind(&last(&h), "image"), 1, "idle is up at the start");

    let service = create_service(&mut h, "Sunday");
    let (song, _) = search(&mut h, "amazing grace")[0].clone();
    let service = insert_song(&mut h, &service, song);
    open_service(&mut h, &service);
    h.sink.take();
    ok(h.app.handle(Request::Live(Command::Next)));
    assert_eq!(count_kind(&last(&h), "image"), 0, "a slide replaces it");

    ok(h.app.handle(Request::Live(Command::EndService)));
    assert_eq!(count_kind(&last(&h), "image"), 1, "and it returns");

    // Black and Logo are one key away and cover it.
    ok(h.app.handle(Request::Live(Command::ToggleBlack)));
    assert_eq!(count_kind(&last(&h), "image"), 0);
    ok(h.app.handle(Request::Live(Command::ToggleBlack)));
    assert_eq!(count_kind(&last(&h), "image"), 1);
    ok(h.app.handle(Request::Live(Command::ToggleLogo)));
    assert_eq!(count_kind(&last(&h), "image"), 0);
}

#[test]
fn a_screens_own_idle_beats_the_default_and_blank_turns_it_off() {
    use lampstand_core::output::IdleScreen;
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let default = blob_with(&h, b"default");
    let own = blob_with(&h, b"own");
    let settings = DeviceSettings {
        idle: Some(IdleScreen::Video {
            blob: default,
            name: "Loop.mp4".into(),
        }),
        ..DeviceSettings::default()
    };
    ok(h.app
        .handle(Request::Settings(SettingsRequest::Put(settings))));
    let mut output = window_output();
    ok(h.app
        .handle(Request::Outputs(OutputRequest::Put(output.clone()))));
    let last = |h: &Harness| shown_scenes(&h.sink).pop().expect("a scene was sent").1;
    let scene = last(&h);
    assert_eq!(
        (count_kind(&scene, "video"), count_kind(&scene, "image")),
        (1, 0)
    );

    output.idle = Some(IdleScreen::Image {
        blob: own,
        name: "Own.png".into(),
    });
    ok(h.app
        .handle(Request::Outputs(OutputRequest::Put(output.clone()))));
    let scene = last(&h);
    assert_eq!(
        (count_kind(&scene, "video"), count_kind(&scene, "image")),
        (0, 1)
    );

    output.idle = Some(IdleScreen::Blank);
    ok(h.app.handle(Request::Outputs(OutputRequest::Put(output))));
    let scene = last(&h);
    assert_eq!(
        (count_kind(&scene, "video"), count_kind(&scene, "image")),
        (0, 0)
    );
}

#[test]
fn an_idle_file_that_is_not_on_this_device_shows_the_plain_background() {
    use lampstand_core::output::IdleScreen;
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let settings = DeviceSettings {
        idle: Some(IdleScreen::Image {
            blob: lampstand_core::id::BlobHash::of(b"never arrived"),
            name: "Gone.png".into(),
        }),
        ..DeviceSettings::default()
    };
    ok(h.app
        .handle(Request::Settings(SettingsRequest::Put(settings))));
    ok(h.app
        .handle(Request::Outputs(OutputRequest::Put(window_output()))));
    let scene = shown_scenes(&h.sink).pop().unwrap().1;
    assert_eq!(count_kind(&scene, "image"), 0);
}

#[test]
fn saving_the_brand_makes_the_default_theme_and_saving_again_updates_it() {
    use lampstand_core::brand::Brand;
    use lampstand_core::geom::Color;
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    let brand = |name: &str, primary| Brand {
        name: name.into(),
        logo: None,
        primary,
        text: None,
        accent: Color::rgb(240, 190, 60),
        font_family: "Noto Serif".into(),
        bug: Default::default(),
    };
    let saved = |h: &mut Harness, b: Brand| match ok(h
        .app
        .handle(Request::Settings(SettingsRequest::SaveBrand(b))))
    {
        Response::Settings(s) => s,
        other => panic!("{other:?}"),
    };
    let first = saved(&mut h, brand("Grace Chapel", Color::rgb(20, 30, 90)));
    let id = first.default_theme.expect("it becomes the default");
    assert_eq!(first.brand_theme, Some(id));
    let themes = list_themes(&mut h);
    assert_eq!(themes.len(), 2);
    assert!(themes
        .iter()
        .any(|t| t.id == id && t.name == "Grace Chapel"));

    let second = saved(&mut h, brand("Grace Church", Color::rgb(90, 20, 30)));
    assert_eq!(second.default_theme, Some(id), "the same theme is updated");
    let themes = list_themes(&mut h);
    assert_eq!(themes.len(), 2);
    assert!(themes
        .iter()
        .any(|t| t.id == id && t.name == "Grace Church"));

    // Deleting it clears the settings that named it.
    ok(h.app
        .handle(Request::Library(LibraryRequest::DeleteTheme(id))));
    let after = match ok(h.app.handle(Request::Settings(SettingsRequest::Get))) {
        Response::Settings(s) => s,
        other => panic!("{other:?}"),
    };
    assert_eq!((after.default_theme, after.brand_theme), (None, None));
    assert!(after.brand.is_some());
}

#[test]
fn a_brand_file_carries_the_church_look_to_another_computer() {
    use lampstand_api::BrandImportMode;
    use lampstand_core::brand::{Brand, BugCorner, BugSize, LogoBug};
    use lampstand_core::geom::Color;
    use lampstand_core::output::IdleScreen;
    let (a_dir, b_dir, files) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    let mut a = open(a_dir.path());
    let logo = blob_with(&a, b"logo");
    let video = blob_with(&a, b"video");
    ok(a.app
        .handle(Request::Settings(SettingsRequest::SaveBrand(Brand {
            name: "Grace Chapel".into(),
            logo: Some(logo),
            primary: Color::rgb(20, 30, 90),
            text: None,
            accent: Color::rgb(240, 190, 60),
            font_family: "Some Font Nobody Has".into(),
            bug: LogoBug {
                enabled: true,
                corner: BugCorner::LowerLeft,
                size: BugSize::Large,
                opacity: 40,
            },
        }))));
    let mut settings = match ok(a.app.handle(Request::Settings(SettingsRequest::Get))) {
        Response::Settings(s) => s,
        other => panic!("{other:?}"),
    };
    settings.idle = Some(IdleScreen::Video {
        blob: video,
        name: "Loop.mp4".into(),
    });
    ok(a.app
        .handle(Request::Settings(SettingsRequest::Put(settings))));
    let path = files
        .path()
        .join("grace.lampstand-brand")
        .to_string_lossy()
        .into_owned();
    let said = match ok(a.app.handle(Request::Library(LibraryRequest::ExportBrand {
        path: path.clone(),
    }))) {
        Response::Text(t) => t,
        other => panic!("{other:?}"),
    };
    assert!(
        said.contains("1 theme") && said.contains("2 picture or video files"),
        "{said}"
    );

    let mut b = open(b_dir.path());
    let contents = match ok(b.app.handle(Request::Library(LibraryRequest::InspectBrand {
        path: path.clone(),
    }))) {
        Response::BrandFile(c) => c,
        other => panic!("{other:?}"),
    };
    assert_eq!(contents.name.as_deref(), Some("Grace Chapel"));
    assert_eq!(contents.themes, ["Grace Chapel"]);
    assert!(contents.has_logo && !contents.font_installed);
    assert_eq!(contents.missing_fonts, ["Some Font Nobody Has"]);
    assert_eq!(contents.idle.as_deref(), Some("Looping video: Loop.mp4"));
    assert_eq!(list_themes(&mut b).len(), 1, "inspecting changes nothing");

    for _ in 0..2 {
        ok(b.app.handle(Request::Library(LibraryRequest::ImportBrand {
            path: path.clone(),
            mode: BrandImportMode::Replace,
        })));
    }
    assert_eq!(
        list_themes(&mut b).len(),
        2,
        "a second import does not duplicate"
    );
    let got = match ok(b.app.handle(Request::Settings(SettingsRequest::Get))) {
        Response::Settings(s) => s,
        other => panic!("{other:?}"),
    };
    assert_eq!(got.brand.as_ref().unwrap().name, "Grace Chapel");
    assert_eq!(
        got.brand.as_ref().unwrap().bug,
        LogoBug {
            enabled: true,
            corner: BugCorner::LowerLeft,
            size: BugSize::Large,
            opacity: 40,
        },
        "the logo bug comes with the brand file"
    );
    assert!(got.default_theme.is_some() && got.default_theme == got.brand_theme);
    assert!(matches!(got.idle, Some(IdleScreen::Video { .. })));
    assert!(b
        .app
        .library()
        .repo()
        .blobs()
        .path(&got.brand.unwrap().logo.unwrap())
        .is_some());

    // Not a brand file.
    let junk = files.path().join("junk.lampstand-brand");
    std::fs::write(&junk, b"nope").unwrap();
    let msg = error_message(b.app.handle(Request::Library(LibraryRequest::InspectBrand {
        path: junk.to_string_lossy().into_owned(),
    })));
    assert!(msg.contains("not a Lampstand brand file"), "{msg}");
}

/// A 40x20 PNG, so the logo is twice as wide as it is high.
fn wide_logo() -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbaImage::from_pixel(40, 20, image::Rgba([240, 190, 60, 255]))
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    bytes
}

/// The rectangle of the image node for `blob` in the last scene shown.
fn bug_in(scene: &Scene, blob: lampstand_core::id::BlobHash) -> Option<lampstand_core::geom::Rect> {
    scene.nodes.iter().find_map(|n| match &n.kind {
        NodeKind::Image(i) if i.blob == blob && i.rect.width < 200.0 => Some(i.rect),
        _ => None,
    })
}

#[test]
fn the_logo_bug_is_drawn_on_audience_screens_and_follows_black_and_the_toggle() {
    use lampstand_core::brand::{Brand, BugCorner, BugSize, LogoBug};
    use lampstand_core::geom::Color;
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(dir.path());
    ok(h.app
        .handle(Request::Outputs(OutputRequest::Put(window_output()))));
    let service = create_service(&mut h, "Sunday");
    let (song, _) = search(&mut h, "holy")[0].clone();
    let service = insert_song(&mut h, &service, song);
    open_service(&mut h, &service);
    ok(h.app.handle(Request::Live(Command::Next)));

    // Nothing to put it on before there is a brand.
    let none = h.app.handle(Request::Settings(SettingsRequest::SetLogoBug(
        LogoBug::default(),
    )));
    assert!(error_message(none).contains("logo"));

    let logo = blob_with(&h, &wide_logo());
    ok(h.app
        .handle(Request::Settings(SettingsRequest::SaveBrand(Brand {
            name: "Grace Chapel".into(),
            logo: Some(logo),
            primary: Color::rgb(20, 30, 90),
            text: None,
            accent: Color::rgb(240, 190, 60),
            font_family: "Noto Serif".into(),
            bug: Default::default(),
        }))));
    h.sink.take();
    let last = |h: &Harness| shown_scenes(&h.sink).pop().expect("a scene was shown").1;

    // Off by default: existing installs do not change.
    ok(h.app.handle(Request::Live(Command::Next)));
    assert!(bug_in(&last(&h), logo).is_none());

    let bug = LogoBug {
        enabled: true,
        ..LogoBug::default()
    };
    ok(h.app
        .handle(Request::Settings(SettingsRequest::SetLogoBug(bug))));
    let scene = last(&h);
    let rect = bug_in(&scene, logo).expect("the bug is on the screen");
    assert!((rect.width / rect.height - 2.0).abs() < 0.01, "{rect:?}");
    assert!((rect.height - 540.0 * 0.07).abs() < 0.5);
    assert!(rect.x > 480.0 && rect.y > 270.0, "lower right: {rect:?}");

    // The next slide keeps it where it was.
    ok(h.app.handle(Request::Live(Command::Next)));
    assert_eq!(bug_in(&last(&h), logo), Some(rect));

    ok(h.app
        .handle(Request::Settings(SettingsRequest::SetLogoBug(LogoBug {
            corner: BugCorner::LowerLeft,
            size: BugSize::Large,
            ..bug
        }))));
    let moved = bug_in(&last(&h), logo).unwrap();
    assert!(moved.x < 480.0 && moved.height > rect.height);

    // Black covers it; turning Black off brings it back.
    ok(h.app.handle(Request::Live(Command::ToggleBlack)));
    assert!(bug_in(&last(&h), logo).is_none());
    ok(h.app.handle(Request::Live(Command::ToggleBlack)));
    assert!(bug_in(&last(&h), logo).is_some());

    // The toggle is saved in the brand.
    ok(h.app
        .handle(Request::Settings(SettingsRequest::SetLogoBug(LogoBug {
            enabled: false,
            ..moved_setting()
        }))));
    assert!(bug_in(&last(&h), logo).is_none());
    let saved = match ok(h.app.handle(Request::Settings(SettingsRequest::Get))) {
        Response::Settings(s) => s.brand.unwrap().bug,
        other => panic!("{other:?}"),
    };
    assert!(!saved.enabled && saved.corner == BugCorner::LowerLeft);
}

fn moved_setting() -> lampstand_core::brand::LogoBug {
    use lampstand_core::brand::{BugCorner, BugSize, LogoBug};
    LogoBug {
        enabled: true,
        corner: BugCorner::LowerLeft,
        size: BugSize::Large,
        opacity: 80,
    }
}

#[test]
fn a_planning_device_refuses_live_control_and_keeps_its_screens_dark() {
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
    assert!(h.app.program().content.is_some(), "on air while presenting");
    assert_eq!(h.host.0.lock().unwrap().open.len(), 1);

    // Another device takes over: the service comes off air and the
    // window closes, but the output stays switched on in its settings.
    h.app.set_presenting(false);
    assert!(h.app.program().content.is_none());
    assert_eq!(h.host.0.lock().unwrap().closed, [output.id]);
    let enabled = match ok(h.app.handle(Request::Outputs(OutputRequest::List))) {
        Response::Outputs(o) => o[0].enabled,
        other => panic!("{other:?}"),
    };
    assert!(enabled, "the screen keeps its setting");

    for command in [Command::GoLive, Command::Next, Command::ToggleBlack] {
        match h.app.handle(Request::Live(command)) {
            Response::Error { message } => assert!(message.contains("planning"), "{message}"),
            other => panic!("expected a refusal, got {other:?}"),
        }
    }
    assert!(matches!(
        h.app
            .handle(Request::Service(ServiceRequest::Open(service.id))),
        Response::Error { .. }
    ));
    assert!(h.app.program().content.is_none());

    // Presenting again brings the window back, with nothing on air.
    h.app.set_presenting(true);
    assert_eq!(h.host.0.lock().unwrap().open.len(), 2);
    open_service(&mut h, &service);
    ok(h.app.handle(Request::Live(Command::Next)));
    assert!(h.app.program().content.is_some());
}
