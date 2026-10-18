//! Importers against hand-written fixtures of public-domain hymns.

use std::path::{Path, PathBuf};

use lampstand_core::song::{SectionKind, Song};
use lampstand_songs::{import_dir, import_file, ImportError, Imported, Report};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

fn only_song(imported: &Imported) -> &Song {
    assert_eq!(imported.songs.len(), 1, "{:?}", imported.songs);
    &imported.songs[0]
}

fn labels(song: &Song) -> Vec<&str> {
    song.sections.iter().map(|s| s.label.as_str()).collect()
}

fn mentions(report: &Report, text: &str) -> bool {
    report.notes.iter().any(|n| n.message.contains(text))
}

#[test]
fn openlyrics_09() {
    let imported = import_file(&fixture("amazing-grace.xml")).unwrap();
    let song = only_song(&imported);

    assert_eq!(song.title, "Amazing Grace");
    assert_eq!(song.alternate_titles, ["Amazing Grace, How Sweet the Sound"]);
    assert_eq!(song.authors, ["John Newton"]);
    assert_eq!(song.copyright.as_deref(), Some("Public Domain"));
    assert_eq!(song.ccli_number, Some(22025));
    assert_eq!(song.key.as_deref(), Some("G"));
    assert_eq!(song.tempo_bpm, Some(72));
    assert_eq!(labels(song), ["V1", "V2", "V3"]);
    assert_eq!(song.arrangement, ["V1", "V2", "V3", "V1"]);

    let v1 = &song.sections[0];
    assert_eq!(v1.kind, SectionKind::Verse);
    assert_eq!(
        v1.lines,
        [
            "[G]Amazing grace! How [C]sweet the [G]sound",
            "That saved a wretch like [D]me!",
            "I once was lost, but now am found;",
            "Was blind, but now I see.",
        ]
    );
    assert_eq!(v1.lyrics()[0], "Amazing grace! How sweet the sound");
    assert_eq!(song.sections[2].lines, ["Through many dangers, toils and snares,", "I have already come;"]);

    let r = &imported.report;
    assert!(mentions(r, "themes not imported"), "{r:?}");
    assert!(mentions(r, "text formatting not imported"), "{r:?}");
    assert!(mentions(r, "comments within lyrics not imported"), "{r:?}");
    assert!(r.notes.iter().all(|n| n.song.as_deref() == Some("Amazing Grace")));
}

#[test]
fn openlyrics_08_keeps_first_language() {
    let imported = import_file(&fixture("library/silent-night.xml")).unwrap();
    let song = only_song(&imported);

    assert_eq!(song.authors, ["Joseph Mohr", "John Freeman Young"]);
    assert_eq!(song.tempo_bpm, None);
    assert_eq!(song.sections.len(), 1);
    assert_eq!(song.sections[0].lines, ["[Bb]Silent night, holy night", "All is calm, all is bright"]);
    assert!(mentions(&imported.report, "language \"de\""), "{:?}", imported.report);
    assert!(mentions(&imported.report, "tempo"), "{:?}", imported.report);
}

#[test]
fn opensong() {
    let imported = import_file(&fixture("library/opensong/Holy Holy Holy")).unwrap();
    let song = only_song(&imported);

    assert_eq!(song.title, "Holy, Holy, Holy");
    assert_eq!(song.alternate_titles, ["Holy Holy Holy Lord God Almighty"]);
    assert_eq!(song.authors, ["Reginald Heber"]);
    assert_eq!(song.key.as_deref(), Some("D"));
    assert_eq!(labels(song), ["V1", "V2"]);
    assert_eq!(song.arrangement, ["V1", "V2", "V1"]);
    assert_eq!(
        song.sections[0].lines,
        [
            "[D]Holy, holy, [Bm]holy! [G]Lord God [D]Almighty!",
            "[A]Early in the [D]morn[E]ing",
            "[A]our song shall rise to Thee;",
        ]
    );
    assert_eq!(
        song.sections[1].lyrics(),
        ["Holy, holy, holy! All the saints adore Thee,", "Casting down their golden crowns", "around the glassy sea;"]
    );

    let r = &imported.report;
    for expected in ["hymn number", "tempo \"Moderate\"", "comments within lyrics", "slide breaks"] {
        assert!(mentions(r, expected), "missing {expected:?} in {r:?}");
    }
    // Empty fields are not worth a note.
    assert!(!mentions(r, "capo") && !mentions(r, "time signature"), "{r:?}");
}

#[test]
fn chordpro() {
    let imported = import_file(&fixture("library/blessed-assurance.cho")).unwrap();
    let song = only_song(&imported);

    assert_eq!(song.title, "Blessed Assurance");
    assert_eq!(song.alternate_titles, ["Jesus Is Mine"]);
    assert_eq!(song.authors, ["Fanny Crosby"]);
    assert_eq!(song.tempo_bpm, Some(100));
    assert_eq!(labels(song), ["V1", "C", "V2"]);
    assert_eq!(song.sections[1].kind, SectionKind::Chorus);
    assert_eq!(song.arrangement, ["V1", "C", "V2", "C"]);
    assert_eq!(song.sections[0].lines[0], "[D]Blessed assurance, [G]Jesus is [D]mine!");
    assert!(mentions(&imported.report, "{time} not imported"), "{:?}", imported.report);
}

#[test]
fn directory_import_reports_each_file() {
    let results = import_dir(&fixture("library")).unwrap();
    let name = |p: &PathBuf| p.file_name().unwrap().to_string_lossy().into_owned();

    let mut ok: Vec<String> = Vec::new();
    let mut failed: Vec<String> = Vec::new();
    for (path, result) in &results {
        match result {
            Ok(imported) => ok.extend(imported.songs.iter().map(|s| s.title.clone())),
            Err(ImportError::UnknownFormat) => failed.push(name(path)),
            Err(e) => panic!("{}: {e}", path.display()),
        }
    }
    ok.sort();
    assert_eq!(ok, ["Blessed Assurance", "Holy, Holy, Holy", "Silent Night"]);
    assert_eq!(failed, ["readme.txt"]);
}
