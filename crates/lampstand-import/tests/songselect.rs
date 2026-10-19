use std::path::PathBuf;

use lampstand_import::songs::songselect;
use lampstand_import::{import_songs, ImportError, SongFormat};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/songselect")
}

fn lines(song: &lampstand_core::song::Song, label: &str) -> Vec<String> {
    song.sections
        .iter()
        .find(|s| s.label == label)
        .unwrap_or_else(|| panic!("no section {label}"))
        .lines
        .clone()
}

#[test]
fn usr_maps_metadata_sections_and_arrangement() {
    let out = songselect::import(&fixtures().join("lamp-for-the-path.usr")).unwrap();
    assert!(out.skipped.is_empty());
    let song = &out.songs[0];
    assert_eq!(song.title, "Lamp for the Path");
    assert_eq!(song.ccli_number, Some(9000001));
    assert_eq!(song.author.as_deref(), Some("Ann Example, Ben Sample"));
    assert_eq!(
        song.copyright.as_deref(),
        Some("2001 Example Songs (Admin. by Example Admin)")
    );
    assert_eq!(song.key.as_deref(), Some("G"));
    let labels: Vec<_> = song.sections.iter().map(|s| s.label.as_str()).collect();
    assert_eq!(labels, ["V1", "C", "V2", "B"]);
    assert_eq!(song.arrangement, ["V1", "C", "V2", "C", "B"]);
    assert_eq!(
        lines(song, "C"),
        ["Shine on, shine on", "We walk by gentle light"]
    );
    assert_eq!(song.arranged().unwrap().len(), 5);
}

#[test]
fn usr_in_windows_1252_decodes() {
    let out = songselect::import(&fixtures().join("cafe-cp1252.usr")).unwrap();
    let song = &out.songs[0];
    assert_eq!(song.title, "Café Song");
    assert_eq!(song.author.as_deref(), Some("José Invented"));
    assert!(song.arrangement.is_empty());
}

#[test]
fn txt_maps_headings_and_footer() {
    let out = songselect::import(&fixtures().join("river-of-peace.txt")).unwrap();
    assert!(out.skipped.is_empty());
    let song = &out.songs[0];
    assert_eq!(song.title, "River of Peace");
    assert_eq!(song.ccli_number, Some(9000004));
    assert_eq!(
        song.author.as_deref(),
        Some("Carol Invented, Dan Fictional")
    );
    assert_eq!(
        song.copyright.as_deref(),
        Some("© 2005 Invented Publishing (Admin. by Example Admin)")
    );
    let labels: Vec<_> = song.sections.iter().map(|s| s.label.as_str()).collect();
    assert_eq!(labels, ["V1", "C", "V2", "B"]);
    assert_eq!(song.arrangement, ["V1", "C", "V2", "C", "B"]);
    assert_eq!(lines(song, "B"), ["Let it run, let it run"]);
    assert!(!song
        .sections
        .iter()
        .flat_map(|s| &s.lines)
        .any(|l| l.contains("CCLI")));
}

#[test]
fn folder_imports_good_files_and_skips_bad_ones() {
    let out = import_songs(SongFormat::SongSelect, &fixtures()).unwrap();
    assert_eq!(out.songs.len(), 3);
    let mut skipped: Vec<_> = out.skipped.iter().map(|s| s.source.as_str()).collect();
    skipped.sort();
    assert_eq!(skipped, ["malformed.usr", "readme.txt"]);
    assert!(out.skipped.iter().all(|s| !s.reason.is_empty()));
}

#[test]
fn single_bad_file_is_skipped_not_an_error() {
    let out = songselect::import(&fixtures().join("malformed.usr")).unwrap();
    assert!(out.songs.is_empty());
    assert_eq!(out.skipped.len(), 1);
}

#[test]
fn other_file_types_are_wrong_format() {
    let err = songselect::import(&fixtures().join("nope.xml")).unwrap_err();
    assert!(matches!(err, ImportError::WrongFormat));
}
