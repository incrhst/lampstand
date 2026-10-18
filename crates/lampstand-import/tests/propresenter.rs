//! ProPresenter importer tests. The fixtures are hand-made in the shape of real
//! exports (Pro6 XML with base64 RTF; Pro7 protobuf), using the public-domain
//! hymn "Amazing Grace".

use std::fs;
use std::path::{Path, PathBuf};

use lampstand_core::song::Song;
use lampstand_import::{import_songs, ImportError, SongFormat};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/propresenter")
        .join(name)
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lampstand-pp-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn import_one(name: &str) -> Song {
    let mut import = import_songs(SongFormat::ProPresenter, &fixture(name)).unwrap();
    assert!(import.skipped.is_empty(), "{:?}", import.skipped);
    assert_eq!(import.songs.len(), 1);
    import.songs.remove(0)
}

fn check_amazing_grace(song: &Song) {
    assert_eq!(song.title, "Amazing Grace");
    assert_eq!(song.author.as_deref(), Some("John Newton"));
    assert_eq!(song.ccli_number, Some(22025));
    assert_eq!(song.copyright.as_deref(), Some("1779 Public Domain"));

    // The empty "Blank" group is dropped and the second "Verse 2" is renamed.
    let labels: Vec<_> = song.sections.iter().map(|s| s.label.as_str()).collect();
    assert_eq!(labels, ["Verse 1", "Verse 2", "Verse 3", "Verse 2 2"]);

    assert_eq!(
        song.sections[0].lines,
        [
            "Amazing grace! How sweet the sound",
            "That saved a wretch like me!",
            "I once was lost, but now am found,",
            "Was blind, but now I see.",
        ]
    );
    assert_eq!(
        song.sections[1].lines[0],
        "’Twas grace that taught my heart to fear,"
    );
    // Two slides in one group are separated by a blank line.
    assert_eq!(
        song.sections[2].lines,
        [
            "Through many dangers, toils and snares",
            "I have already come;",
            "",
            "’Tis grace hath brought me safe thus far,",
            "And grace will lead me home.",
        ]
    );
    assert_eq!(song.sections[3].lines, ["Lord, I believe it"]);

    // The selected arrangement ("Default") repeats verse 1 at the end.
    assert_eq!(
        song.arrangement,
        ["Verse 1", "Verse 2", "Verse 3", "Verse 1"]
    );
}

#[test]
fn pro6_song() {
    check_amazing_grace(&import_one("Amazing Grace.pro6"));
}

#[test]
fn pro7_song() {
    let song = import_one("Amazing Grace.pro");
    check_amazing_grace(&song);
    assert_eq!(song.key.as_deref(), Some("G"));
}

#[test]
fn folder_import_finds_both_and_skips_bad_files_with_a_reason() {
    let dir = scratch("folder");
    fs::create_dir_all(dir.join("nested")).unwrap();
    for name in ["Amazing Grace.pro6", "Amazing Grace.pro"] {
        fs::copy(fixture(name), dir.join("nested").join(name)).unwrap();
    }
    fs::write(dir.join("broken.pro6"), "<RVPresentationDocument><groups>").unwrap();
    fs::write(dir.join("other.pro6"), "<html/>").unwrap();
    fs::write(dir.join("broken.pro"), [0xff, 0xff, 0xff]).unwrap();
    fs::write(dir.join("notes.txt"), "ignored").unwrap();

    let import = import_songs(SongFormat::ProPresenter, &dir).unwrap();
    assert_eq!(import.songs.len(), 2);
    let mut skipped: Vec<_> = import.skipped.iter().map(|s| s.source.as_str()).collect();
    skipped.sort();
    assert_eq!(skipped, ["broken.pro", "broken.pro6", "other.pro6"]);
    assert!(import.skipped.iter().all(|s| !s.reason.is_empty()));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn without_a_selected_arrangement_sections_keep_written_order() {
    let dir = scratch("noarr");
    let xml = fs::read_to_string(fixture("Amazing Grace.pro6")).unwrap();
    let xml = xml.replace(
        "selectedArrangementID=\"00000000-0000-4000-8000-000000000100\"",
        "selectedArrangementID=\"\"",
    );
    fs::write(dir.join("Song.pro6"), xml).unwrap();
    let import = import_songs(SongFormat::ProPresenter, &dir.join("Song.pro6")).unwrap();
    assert_eq!(import.songs[0].title, "Song");
    assert!(import.songs[0].arrangement.is_empty());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_file_of_another_format_is_wrong_format() {
    let dir = scratch("wrong");
    fs::write(dir.join("a.txt"), "x").unwrap();
    let err = import_songs(SongFormat::ProPresenter, &dir.join("a.txt")).unwrap_err();
    assert!(matches!(err, ImportError::WrongFormat));
    let _ = fs::remove_dir_all(dir);
}
