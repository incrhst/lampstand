//! EasyWorship importer. The EW7 fixtures are real SQLite files; the EW6 ones
//! are a Paradox table and memo file built by `fixtures/easyworship/make_fixtures.py`
//! from the layout OpenLP's importer reads, not exports from EasyWorship, so
//! they prove the reader matches that layout and nothing more.

use std::path::PathBuf;

use lampstand_import::{import_songs, ImportError, SongFormat};

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/easyworship")
        .join(rel)
}

fn labels(song: &lampstand_core::song::Song) -> Vec<&str> {
    song.sections.iter().map(|s| s.label.as_str()).collect()
}

#[test]
fn ew7_sqlite_with_sibling_words_db() {
    let r = import_songs(SongFormat::EasyWorship, &fixture("ew7/Songs.db")).unwrap();
    assert_eq!(r.songs.len(), 2);

    let grace = &r.songs[0];
    assert_eq!(grace.title, "Amazing Grace");
    assert_eq!(grace.author.as_deref(), Some("John Newton / Edwin Excell"));
    assert_eq!(grace.copyright.as_deref(), Some("Public Domain"));
    assert_eq!(grace.ccli_number, Some(22025));
    assert_eq!(labels(grace), ["Verse 1", "Verse 2", "Chorus 1", "Verse 3"]);
    assert_eq!(
        grace.sections[0].lines,
        [
            "Amazing grace, how sweet the sound",
            "That saved a wretch like me",
            "I once was lost, but now am found",
            "Was blind but now I see"
        ]
    );
    // 舖? is a left single quote.
    assert_eq!(
        grace.sections[1].lines[0],
        "\u{2018}Twas grace that taught my heart to fear"
    );
    // The closing "Chorus" with no text is a repeat, not a new section.
    assert_eq!(
        grace.arrangement,
        ["Verse 1", "Verse 2", "Chorus 1", "Verse 3", "Chorus 1"]
    );
    assert_eq!(grace.arranged().unwrap().len(), 5);

    let vision = &r.songs[1];
    assert_eq!(vision.ccli_number, Some(30939));
    assert_eq!(
        vision.copyright.as_deref(),
        Some("Public Domain, Administered by Example Admin")
    );
    // A lyric that begins "End..." is not a heading.
    assert_eq!(labels(vision), ["Verse 1", "Verse 2"]);
    assert_eq!(vision.sections[1].lines[0], "Endless love");
    assert!(vision.arrangement.is_empty());

    assert_eq!(r.skipped.len(), 1);
    assert_eq!(r.skipped[0].source, "Songs.db: Empty Song");
    assert_eq!(r.skipped[0].reason, "no song text found");
}

#[test]
fn ew7_folder_is_searched_for_songs_db() {
    let r = import_songs(SongFormat::EasyWorship, &fixture("ew7")).unwrap();
    assert_eq!(r.songs.len(), 2);
}

#[test]
fn ew7_song_words_table_in_the_same_database() {
    let r = import_songs(SongFormat::EasyWorship, &fixture("ew7-inline/Songs.db")).unwrap();
    assert_eq!(r.songs.len(), 1);
    let s = &r.songs[0];
    assert_eq!(s.title, "Café Song");
    assert_eq!(s.ccli_number, Some(1234567));
    // Hex escape (windows-1252) and \u escape (Cyrillic й).
    assert_eq!(s.sections[0].lines, ["Line café one", "Line йtwo"]);
}

#[test]
fn ew6_paradox_with_memo_file() {
    let r = import_songs(SongFormat::EasyWorship, &fixture("ew6/Songs.DB")).unwrap();
    assert_eq!(r.songs.len(), 3);

    let holy = &r.songs[0];
    assert_eq!(holy.title, "Holy Holy Holy");
    assert_eq!(holy.author.as_deref(), Some("Reginald Heber"));
    assert_eq!(holy.copyright.as_deref(), Some("Public Domain"));
    assert_eq!(holy.ccli_number, Some(12345));
    assert_eq!(labels(holy), ["Verse 1", "Chorus 1"]);
    assert_eq!(holy.arrangement, ["Verse 1", "Chorus 1", "Chorus 1"]);

    let cafe = &r.songs[1];
    assert_eq!(cafe.title, "Café Song");
    assert_eq!(
        cafe.copyright.as_deref(),
        Some("(c) 1999, Administered by Example Admin")
    );
    assert_eq!(
        cafe.sections[0].lines,
        ["Café and naïve words", "On two lines"]
    );
    assert_eq!(cafe.ccli_number, None);

    let fount = &r.songs[2];
    assert_eq!(fount.title, "Come Thou Fount");
    assert_eq!(fount.ccli_number, Some(20000));
    assert_eq!(
        fount.sections[0].lines[0],
        "Come, thou fount of every blessing"
    );

    assert_eq!(r.skipped.len(), 1);
    assert_eq!(r.skipped[0].source, "Songs.DB: No Words");
}

#[test]
fn ew6_folder_is_searched() {
    let r = import_songs(SongFormat::EasyWorship, &fixture("ew6")).unwrap();
    assert_eq!(r.songs.len(), 3);
}

#[test]
fn not_easyworship_is_wrong_format() {
    // A text file, a SQLite database with no song table, and an empty folder.
    let dir = fixture("");
    assert!(matches!(
        import_songs(SongFormat::EasyWorship, &dir.join("make_fixtures.py")),
        Err(ImportError::WrongFormat)
    ));
    assert!(matches!(
        import_songs(SongFormat::EasyWorship, &fixture("ew7/SongWords.db")),
        Err(ImportError::WrongFormat)
    ));
}

#[test]
fn ew6_without_memo_file_is_unreadable() {
    let dir = std::env::temp_dir().join(format!("lampstand-ew6-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(fixture("ew6/Songs.DB"), dir.join("Songs.DB")).unwrap();
    let r = import_songs(SongFormat::EasyWorship, &dir.join("Songs.DB"));
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(matches!(r, Err(ImportError::Unreadable(m)) if m.contains(".MB")));
}

#[test]
fn truncated_paradox_is_an_error_not_a_panic() {
    let bytes = std::fs::read(fixture("ew6/Songs.DB")).unwrap();
    let dir = std::env::temp_dir().join(format!("lampstand-ew6t-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(fixture("ew6/Songs.MB"), dir.join("Songs.MB")).unwrap();
    std::fs::write(dir.join("Songs.DB"), &bytes[..0x800 + 300]).unwrap();
    let r = import_songs(SongFormat::EasyWorship, &dir.join("Songs.DB"));
    std::fs::remove_dir_all(&dir).unwrap();
    let r = r.unwrap();
    assert!(r.songs.len() < 3);
    assert!(!r.skipped.is_empty());
}
