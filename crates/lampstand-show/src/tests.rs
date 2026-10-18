use crate::protocol::{Command, CueView, ItemKind, Mode, Pos};
use crate::{Item, Library, Service, Show, Source};

const AMAZING_GRACE: &str = include_str!("../../lampstand-songs/tests/fixtures/amazing-grace.xml");
const JOHN: &str = include_str!("../../lampstand-scripture/tests/fixtures/JHN.usfm");

fn library() -> (tempfile::TempDir, Library, String) {
    let dir = tempfile::tempdir().unwrap();
    let (mut lib, warnings) = Library::open(dir.path()).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    lib.import_songs(&[("amazing-grace.xml".into(), AMAZING_GRACE.as_bytes().to_vec())]).unwrap();
    lib.import_bible("JHN.usfm", JOHN.as_bytes(), Some("KJV")).unwrap();
    let song = lib.search_songs("amazng grace", 1).pop().expect("typo-tolerant search finds the song");
    (dir, lib, song)
}

fn service(song: &str) -> Service {
    Service {
        name: "Sunday".into(),
        items: vec![
            Item { id: "a".into(), source: Source::Logo },
            Item { id: "b".into(), source: Source::Song { song: song.into() } },
            Item { id: "c".into(), source: Source::Scripture { reference: "Jn 3:16-18".into(), translation: "kjv".into() } },
            Item { id: "d".into(), source: Source::Deck { deck: "nope".into() } },
        ],
    }
}

#[test]
fn items_expand_into_cues() {
    let (_d, lib, song) = library();
    let show = Show::new(service(&song), &lib);
    let s = show.snapshot(None);
    assert_eq!(s.items.len(), 4);
    assert_eq!(s.items[1].kind, ItemKind::Song);
    assert!(s.items[1].cues.len() >= 2, "verses become cues: {:?}", s.items[1].cues);
    assert!(s.items[1].problem.is_none());
    // Scripture: three long KJV verses don't fit one 40×6 slide.
    assert!(s.items[2].cues.len() >= 2, "{:?}", s.items[2].cues);
    assert!(s.items[2].title.starts_with("John 3:16"), "{}", s.items[2].title);
    // A deck that isn't in the library is reported, not fatal.
    assert!(s.items[3].problem.is_some());
    assert!(s.items[3].cues.is_empty());
    match &show.snapshot(None).preview_cue {
        Some(CueView::Text { label, .. }) => assert_eq!(label, "Logo"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn song_cues_carry_the_copyright_footer() {
    let (_d, lib, song) = library();
    let mut show = Show::new(service(&song), &lib);
    show.apply(&Command::GoTo { pos: Pos { item: 1, cue: 0 } }, 1);
    match show.snapshot(None).live_cue {
        Some(CueView::Text { lines, footer, .. }) => {
            assert!(!lines.is_empty() && lines.len() <= 4);
            assert!(footer.unwrap().starts_with("Amazing Grace"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn next_runs_through_the_service_and_skips_empty_items() {
    let (_d, lib, song) = library();
    let mut show = Show::new(service(&song), &lib);
    let s = show.snapshot(None);
    let total: usize = s.items.iter().map(|i| i.cues.len()).sum();
    // Nothing live: next takes preview (the logo) live.
    show.apply(&Command::Next, 10);
    assert_eq!(show.snapshot(None).live, Some(Pos { item: 0, cue: 0 }));
    assert_eq!(show.snapshot(None).mode, Mode::Logo, "a live logo item shows the logo");
    for _ in 1..total + 3 {
        show.apply(&Command::Next, 20);
    }
    let last = show.snapshot(None);
    // The broken deck has no cues, so the end of the service is the last scripture cue.
    assert_eq!(last.live, Some(Pos { item: 2, cue: last.items[2].cues.len() - 1 }));
    assert_eq!(last.service_started_at, Some(10));
    assert_eq!(last.next_cue, None);
    // And back across the item boundary.
    for _ in 0..last.items[2].cues.len() {
        show.apply(&Command::Prev, 30);
    }
    let back = show.snapshot(None);
    assert_eq!(back.live, Some(Pos { item: 1, cue: back.items[1].cues.len() - 1 }));
    assert_eq!(back.item_started_at, Some(30));
}

#[test]
fn go_live_advances_preview_and_modes_toggle() {
    let (_d, lib, song) = library();
    let mut show = Show::new(service(&song), &lib);
    show.apply(&Command::Select { pos: Pos { item: 1, cue: 0 } }, 0);
    show.apply(&Command::GoLive, 0);
    let s = show.snapshot(None);
    assert_eq!(s.live, Some(Pos { item: 1, cue: 0 }));
    assert_eq!(s.preview, Some(Pos { item: 1, cue: 1 }));
    assert_eq!(s.next_cue, s.preview_cue);
    show.apply(&Command::Mode { mode: Mode::Black }, 0);
    assert_eq!(show.snapshot(None).mode, Mode::Black);
    show.apply(&Command::Mode { mode: Mode::Black }, 0);
    assert_eq!(show.snapshot(None).mode, Mode::Normal);
    show.apply(&Command::Mode { mode: Mode::Black }, 0);
    show.apply(&Command::GoLive, 0);
    assert_eq!(show.snapshot(None).mode, Mode::Normal, "taking a cue live leaves black");
    let before = show.snapshot(None);
    show.apply(&Command::Select { pos: Pos { item: 9, cue: 0 } }, 0);
    assert_eq!(show.snapshot(None).preview, before.preview, "an invalid select changes nothing");
}

#[test]
fn editing_the_order_keeps_live_on_its_item() {
    let (_d, lib, song) = library();
    let mut show = Show::new(service(&song), &lib);
    show.apply(&Command::GoTo { pos: Pos { item: 2, cue: 1 } }, 0);
    let mut s = service(&song);
    s.items.remove(0); // the logo goes; scripture moves up to index 1
    show.set_service(s.clone(), &lib);
    assert_eq!(show.snapshot(None).live, Some(Pos { item: 1, cue: 1 }));
    s.items.retain(|i| i.id != "c");
    show.set_service(s, &lib);
    assert_eq!(show.snapshot(None).live, None, "removing the live item clears live");
}

#[test]
fn bible_files_of_one_translation_merge() {
    let (dir, mut lib, _) = library();
    let psalms = include_str!("../../lampstand-scripture/tests/fixtures/PSA.usfm");
    lib.import_bible("PSA.usfm", psalms.as_bytes(), Some("KJV")).unwrap();
    for l in [&lib, &Library::open(dir.path()).unwrap().0] {
        let kjv = l.bible("KJV").unwrap();
        assert!(kjv.passage(&"Jn 3:16".parse().unwrap()).is_ok(), "John kept");
        assert!(kjv.passage(&"Ps 23:1".parse().unwrap()).is_ok(), "Psalms added");
    }
}

#[test]
fn library_reloads_from_disk() {
    let (dir, lib, song) = library();
    drop(lib);
    let (lib, warnings) = Library::open(dir.path()).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(lib.song(&song).is_some());
    let kjv = lib.bible("KJV").expect("USFM keeps the abbreviation it was imported with");
    assert!(kjv.verse_count() >= 3);
}
