//! Pasting lyrics: reading them, checking before saving, and getting the
//! song into the library and the service.

use std::sync::{Arc, Mutex};

use lampstand_api::{Event, LibraryRequest, ParsedSong, Request, Response, ServiceRequest};
use lampstand_app::handler::{App, Placement, WindowHost};
use lampstand_core::output::OutputId;
use lampstand_core::service::ItemKind;
use lampstand_layout::Fonts;
use lampstand_render::output::DisplayInfo;
use lampstand_render::{OutputCommand, OutputSink};

struct Sink;
impl OutputSink for Sink {
    fn send(&self, _: OutputId, _: OutputCommand) {}
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

fn app() -> (App, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let events: Arc<Mutex<Vec<Event>>> = Arc::default();
    let app = App::open(
        dir.path(),
        Fonts::bundled_only(),
        Arc::new(Sink),
        Box::new(Host),
        Arc::new(move |e| events.lock().unwrap().push(e)),
    )
    .unwrap();
    (app, dir)
}

fn parse(app: &mut App, text: &str, title: Option<&str>) -> Result<ParsedSong, String> {
    match app.handle(Request::Library(LibraryRequest::ParseLyrics {
        text: text.into(),
        title: title.map(str::to_string),
    })) {
        Response::ParsedSong(p) => Ok(p),
        Response::Error { message } => Err(message),
        other => panic!("{other:?}"),
    }
}

const WORDS: &str = "Verse 1\nFirst line of the verse\nSecond line of the verse\n\nChorus\nThe chorus words\nSung twice\n\nChorus\n";

#[test]
fn pasted_lyrics_are_read_without_saving_anything() {
    let (mut app, _dir) = app();
    let before = app.library().songs().unwrap().len();
    let p = parse(&mut app, WORDS, Some("A New Song")).unwrap();
    assert_eq!(p.song.title, "A New Song");
    assert_eq!(
        p.song
            .sections
            .iter()
            .map(|s| s.label.as_str())
            .collect::<Vec<_>>(),
        ["V1", "C"]
    );
    assert_eq!(p.song.arrangement, ["V1", "C", "C"]);
    assert_eq!(p.same_title, None);
    assert_eq!(
        app.library().songs().unwrap().len(),
        before,
        "reading is not saving"
    );
}

#[test]
fn a_song_with_the_same_title_is_pointed_out() {
    let (mut app, _dir) = app();
    let existing = app.library().songs().unwrap().into_iter().next().unwrap();
    let p = parse(&mut app, WORDS, Some(&existing.title.to_uppercase())).unwrap();
    assert_eq!(p.same_title, Some(existing.id));
}

#[test]
fn nothing_to_read_comes_back_as_words_not_a_crash() {
    let (mut app, _dir) = app();
    assert_eq!(
        parse(&mut app, "   \n", None).unwrap_err(),
        "Paste the words of a song."
    );
}

#[test]
fn a_checked_song_is_saved_found_and_put_in_the_service() {
    let (mut app, _dir) = app();
    let mut p = parse(&mut app, WORDS, Some("A New Song")).unwrap();
    // The person fills in what the paste cannot know.
    p.song.author = Some("A. Writer".into());
    p.song.copyright = Some("© 2020 A Publisher".into());
    p.song.ccli_number = Some(1234567);
    p.song.key = Some("C".into());
    assert!(matches!(
        app.handle(Request::Library(LibraryRequest::PutSong(p.song.clone()))),
        Response::Ok
    ));
    match app.handle(Request::Library(LibraryRequest::SearchSongs {
        query: "New Song".into(),
        limit: 5,
    })) {
        Response::Songs(hits) => assert!(hits.iter().any(|h| h.song == p.song.id)),
        other => panic!("{other:?}"),
    }
    match app.handle(Request::Library(LibraryRequest::GetSong(p.song.id))) {
        Response::Song(s) => {
            assert_eq!(s.author.as_deref(), Some("A. Writer"));
            assert_eq!(s.ccli_number, Some(1234567));
            assert_eq!(s.arrangement, ["V1", "C", "C"]);
        }
        other => panic!("{other:?}"),
    }
    let service = match app.handle(Request::Service(ServiceRequest::Create {
        title: "Sunday".into(),
        starts_at: None,
        time_zone: None,
    })) {
        Response::Service(s) => s,
        other => panic!("{other:?}"),
    };
    let service = match app.handle(Request::Service(ServiceRequest::InsertItem {
        service: service.id,
        index: usize::MAX,
        kind: ItemKind::Song {
            song: p.song.id,
            arrangement: None,
        },
    })) {
        Response::Service(s) => s,
        other => panic!("{other:?}"),
    };
    assert!(service
        .ordered()
        .iter()
        .any(|(_, i)| matches!(&i.kind, ItemKind::Song { song, .. } if *song == p.song.id)));
    // And it can go on the screens: the sung order is three slides' worth.
    match app.handle(Request::Service(ServiceRequest::Open(service.id))) {
        Response::Loaded(l) => assert!(!l.items[0].slides.is_empty()),
        other => panic!("{other:?}"),
    }
}
