use std::path::PathBuf;

use lampstand_import::songs::openlyrics::{import, parse_song};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/openlyrics")
        .join(name)
}

fn labels(song: &lampstand_core::song::Song) -> Vec<&str> {
    song.sections.iter().map(|s| s.label.as_str()).collect()
}

#[test]
fn amazing_grace_metadata() {
    let out = import(&fixture("amazing-grace.xml")).unwrap();
    assert!(out.skipped.is_empty(), "{:?}", out.skipped);
    let song = &out.songs[0];
    assert_eq!(song.title, "Amazing Grace");
    assert_eq!(song.author.as_deref(), Some("John Newton, Edwin O. Excell"));
    assert_eq!(song.copyright.as_deref(), Some("Public Domain"));
    assert_eq!(song.ccli_number, Some(4624));
    assert_eq!(song.key.as_deref(), Some("G"));
    assert_eq!(song.tempo, Some(72));
}

#[test]
fn verse_names_become_labels_and_order_is_the_arrangement() {
    let song = import(&fixture("amazing-grace.xml"))
        .unwrap()
        .songs
        .remove(0);
    assert_eq!(labels(&song), ["V1", "V2", "C", "V3", "E"]);
    assert_eq!(song.arrangement, ["V1", "V2", "C", "V3", "C", "E"]);
    assert_eq!(song.arranged().unwrap().len(), 6);
}

#[test]
fn lines_split_on_br_with_inline_chords_and_format_tags_dropped() {
    let song = import(&fixture("amazing-grace.xml"))
        .unwrap()
        .songs
        .remove(0);
    assert_eq!(
        song.sections[0].lines,
        [
            "[G]Amazing grace, how [C]sweet the [G]sound",
            "That saved a wretch like me",
            "I once was lost, but now am found",
        ]
    );
}

#[test]
fn multiple_lines_blocks_join_one_section() {
    let song = import(&fixture("amazing-grace.xml"))
        .unwrap()
        .songs
        .remove(0);
    assert_eq!(
        song.sections[1].lines,
        [
            "'Twas grace that taught my heart to fear,",
            "and grace my fears relieved",
            "How precious did that grace appear",
            "The hour I first believed",
        ]
    );
}

#[test]
fn entities_are_decoded() {
    let song = import(&fixture("amazing-grace.xml"))
        .unwrap()
        .songs
        .remove(0);
    assert_eq!(
        song.sections[2].lines,
        ["My chains are gone & I\u{2019}ve been set free"]
    );
}

#[test]
fn bridge_and_pre_chorus_labels_without_numbers() {
    let song = import(&fixture("minimal.xml")).unwrap().songs.remove(0);
    assert_eq!(labels(&song), ["V1", "B", "P"]);
    assert!(song.arrangement.is_empty());
    assert_eq!(song.author, None);
    assert_eq!(song.ccli_number, None);
}

#[test]
fn folder_import_skips_bad_files_with_reasons() {
    let out = import(&fixture("")).unwrap();
    let mut titles: Vec<_> = out.songs.iter().map(|s| s.title.as_str()).collect();
    titles.sort();
    assert_eq!(titles, ["Amazing Grace", "Doxology"]);
    let mut skipped: Vec<_> = out.skipped.iter().map(|s| s.source.as_str()).collect();
    skipped.sort();
    assert_eq!(skipped, ["broken.xml", "no-title.xml", "not-a-song.xml"]);
    assert!(out.skipped.iter().all(|s| !s.reason.is_empty()));
}

#[test]
fn a_malformed_single_file_is_skipped_not_an_error() {
    let out = import(&fixture("broken.xml")).unwrap();
    assert!(out.songs.is_empty());
    assert_eq!(out.skipped.len(), 1);
}

#[test]
fn missing_path_is_an_io_error() {
    assert!(import(&fixture("nope.xml")).is_err());
}

#[test]
fn parse_song_handles_chord_text_and_unknown_order_entries() {
    let xml = r#"<song><properties><titles><title>T</title></titles>
        <verseOrder>v1 x9 v1</verseOrder></properties>
        <lyrics><verse name="v1"><lines><chord name="Am"/>One <comment>ignore me</comment>two</lines></verse></lyrics></song>"#;
    let song = parse_song(xml).unwrap();
    assert_eq!(song.sections[0].lines, ["[Am]One two"]);
    assert_eq!(song.arrangement, ["V1", "V1"]);
}

#[test]
fn parse_song_rejects_other_xml() {
    assert!(parse_song("<foo/>").is_err());
    assert!(parse_song("").is_err());
}
