use std::path::PathBuf;

use lampstand_import::{import_songs, SongFormat};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/opensong")
        .join(name)
}

fn import(name: &str) -> lampstand_import::SongImport {
    import_songs(SongFormat::OpenSong, &fixture(name)).unwrap()
}

#[test]
fn single_file_metadata_and_arrangement() {
    let out = import("Amazing Grace");
    assert!(out.skipped.is_empty());
    let song = &out.songs[0];
    assert_eq!(song.title, "Amazing Grace");
    assert_eq!(song.author.as_deref(), Some("John Newton"));
    assert_eq!(song.copyright.as_deref(), Some("Public Domain"));
    assert_eq!(song.ccli_number, Some(22025));
    assert_eq!(song.key.as_deref(), Some("G"));
    assert_eq!(song.tempo, Some(72));
    assert_eq!(song.arrangement, ["V1", "C", "V2", "C"]);
    let labels: Vec<_> = song.sections.iter().map(|s| s.label.as_str()).collect();
    assert_eq!(labels, ["V1", "C", "V2"]);
    assert_eq!(song.arranged().unwrap().len(), 4);
}

#[test]
fn chords_merge_by_column_and_comments_drop() {
    let song = &import("Amazing Grace").songs[0];
    assert_eq!(
        song.sections[0].lines,
        [
            "[G]Amazing grace, [G7]how sweet [C]the sound",
            "[G]That saved a [Em]wretch like [D]me",
        ]
    );
    // Entities are decoded; chords past the start of the line land mid-line.
    assert_eq!(
        song.sections[1].lines,
        [
            "I once [C]was lost & [G]now am found",
            "Was blind, but now I see"
        ]
    );
    assert_eq!(
        song.sections[2].lines[0],
        "'Twas grace that taught my heart to fear,"
    );
}

#[test]
fn multi_verse_lines_split_into_sections() {
    let out = import("Hymns/Holy Holy Holy");
    let song = &out.songs[0];
    let labels: Vec<_> = song.sections.iter().map(|s| s.label.as_str()).collect();
    assert_eq!(labels, ["V1", "V2", "V3"]);
    assert_eq!(
        song.sections[1].lines,
        [
            "Holy, holy, holy! All the saints adore Thee,",
            "Early in the morning our song shall rise to Thee;",
            "",
            "God in three Persons, blessed Trinity!",
        ]
    );
    assert_eq!(song.arrangement, ["V1", "V2", "V3"]);
}

#[test]
fn folder_recurses_and_bad_files_are_skipped() {
    let out = import("");
    let mut titles: Vec<_> = out.songs.iter().map(|s| s.title.as_str()).collect();
    titles.sort();
    assert_eq!(titles, ["Amazing Grace", "Holy, Holy, Holy"]);
    let mut skipped: Vec<_> = out.skipped.iter().map(|s| s.source.as_str()).collect();
    skipped.sort();
    assert_eq!(skipped, ["Broken", "Empty", "Other", "Readme"]);
    assert!(out.skipped.iter().all(|s| !s.reason.is_empty()));
}

#[test]
fn bad_single_file_is_skipped_not_an_error() {
    let out = import("Bad/Readme");
    assert!(out.songs.is_empty());
    assert_eq!(out.skipped.len(), 1);
}

#[test]
fn missing_path_is_an_io_error() {
    assert!(import_songs(SongFormat::OpenSong, &fixture("nope")).is_err());
}

/// An OpenSongApp backup (`.osb`): a zip of song files, the format
/// worshipleaderapp.com publishes its song database in.
#[test]
fn opensongapp_backup_zip() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/opensong_backup/hymns.osb");
    let out = import_songs(SongFormat::OpenSong, &path).unwrap();
    let mut titles: Vec<_> = out.songs.iter().map(|s| s.title.as_str()).collect();
    titles.sort();
    assert_eq!(titles, ["Amazing Grace", "Holy, Holy, Holy"]);
    assert_eq!(out.skipped.len(), 1, "{:?}", out.skipped);
    assert_eq!(out.skipped[0].source, "Bad/Broken");
}
