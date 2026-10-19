use std::path::PathBuf;

use lampstand_import::songs::freeshow;
use lampstand_import::ImportError;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/freeshow")
        .join(name)
}

#[test]
fn imports_a_show_file() {
    let out = freeshow::import(&fixture("amazing-grace.show")).unwrap();
    assert!(out.skipped.is_empty());
    let song = &out.songs[0];
    assert_eq!(song.title, "Amazing Grace");
    assert_eq!(song.author.as_deref(), Some("John Newton"));
    assert_eq!(song.copyright.as_deref(), Some("Public Domain"));
    assert_eq!(song.ccli_number, Some(22025));
    assert_eq!(song.key.as_deref(), Some("G"));

    let labels: Vec<_> = song.sections.iter().map(|s| s.label.as_str()).collect();
    // Sections are in first-use order.
    assert_eq!(labels, ["V1", "C", "V2", "E"]);
    // Chords come through as ChordPro.
    assert_eq!(
        song.sections[0].lines[0],
        "[G]Amazing [G7]grace, how [C]sweet the sound"
    );
    // A continuation slide is folded into its parent.
    assert_eq!(song.sections[2].lines.len(), 3);
    // Layout order with repeats; the disabled slide and blank slide are left out.
    assert_eq!(song.arrangement, ["V1", "C", "V2", "C", "E"]);
    assert!(song.arranged().is_ok());
}

#[test]
fn falls_back_to_first_layout_and_group_names() {
    let out = freeshow::import(&fixture("sub/custom.show")).unwrap();
    let song = &out.songs[0];
    let labels: Vec<_> = song.sections.iter().map(|s| s.label.as_str()).collect();
    assert_eq!(labels, ["V1", "V2", "Coda"]);
    assert_eq!(song.arrangement, labels);
    assert_eq!(song.ccli_number, None);
}

#[test]
fn folder_skips_malformed_files() {
    let out = freeshow::import(&fixture("")).unwrap();
    let mut titles: Vec<_> = out.songs.iter().map(|s| s.title.as_str()).collect();
    titles.sort();
    assert_eq!(titles, ["Amazing Grace", "Custom Song"]);
    assert_eq!(out.skipped.len(), 1);
    assert_eq!(out.skipped[0].source, "broken.show");
}

#[test]
fn malformed_single_file_is_skipped_not_failed() {
    let out = freeshow::import(&fixture("broken.show")).unwrap();
    assert!(out.songs.is_empty());
    assert_eq!(out.skipped.len(), 1);
}

#[test]
fn empty_folder_is_wrong_format() {
    let dir = std::env::temp_dir().join(format!("lampstand-fs-empty-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let r = freeshow::import(&dir);
    std::fs::remove_dir_all(&dir).ok();
    assert!(matches!(r, Err(ImportError::WrongFormat)));
}
