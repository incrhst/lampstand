//! T12: USFM Bible importer. Fixtures are trimmed books of the World
//! English Bible (public domain) from eBible.org.

use std::fs;
use std::path::{Path, PathBuf};

use lampstand_core::scripture::{Licence, Translation};
use lampstand_import::{import_bible, BibleFormat, BibleImport, ImportError};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/usfm")
}

fn web() -> BibleImport {
    import_bible(BibleFormat::Usfm, &fixtures()).expect("fixture imports")
}

fn verse(i: &BibleImport, book: u8, chapter: usize, verse: usize) -> &str {
    &i.text.books[usize::from(book) - 1][chapter - 1][verse - 1]
}

/// A temporary folder holding the given (file name, contents) pairs.
fn folder(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lampstand-usfm-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    for (file, body) in files {
        fs::write(dir.join(file), body).unwrap();
    }
    dir
}

#[test]
fn translation_metadata() {
    let i = web();
    let Translation {
        name,
        abbreviation,
        language,
        licence,
        ..
    } = &i.translation;
    assert_eq!(name, "World English Bible");
    assert_eq!(abbreviation, "WEB");
    assert_eq!(language, "en");
    assert_eq!(*licence, Licence::PublicDomain);
}

#[test]
fn books_land_in_canonical_slots() {
    let i = web();
    assert_eq!(i.text.books.len(), 66);
    let present: Vec<usize> = (0..66).filter(|b| !i.text.books[*b].is_empty()).collect();
    assert_eq!(present, vec![7, 42, 63]); // Ruth, John, 3 John
    assert_eq!(i.text.books[7].len(), 2); // fixture is trimmed to Ruth 1-2
    assert_eq!(i.text.books[42].len(), 3); // John 1-3
    assert_eq!(i.text.books[63].len(), 1);
    assert_eq!(i.text.books[42][0].len(), 51);
    assert_eq!(i.text.books[63][0].len(), 14);
}

#[test]
fn strong_attributes_are_dropped() {
    let i = web();
    assert!(
        verse(&i, 43, 1, 1).starts_with("In the beginning was the Word, and the Word was with God")
    );
    assert!(verse(&i, 43, 1, 1).ends_with("the Word was God."));
    assert_eq!(
        verse(&i, 8, 1, 1).split(',').next().unwrap(),
        "In the days when the judges judged"
    );
}

#[test]
fn footnotes_and_cross_references_are_dropped() {
    let i = web();
    // Ruth 1:15 carries a footnote; it must not leak into the text.
    let v = verse(&i, 8, 1, 15);
    assert!(
        v.starts_with("She said, “Behold, your sister-in-law has gone back"),
        "{v}"
    );
    for chapters in &i.text.books {
        for verses in chapters {
            for v in verses {
                for stray in ["\\", "strong=", "|", "means look at"] {
                    assert!(!v.contains(stray), "{stray:?} left in {v:?}");
                }
            }
        }
    }
}

#[test]
fn words_of_jesus_are_plain_text() {
    let i = web();
    let v = verse(&i, 43, 3, 3);
    assert!(v.contains("Most certainly"), "{v}");
    assert!(!v.contains("wj"));
}

#[test]
fn headings_and_front_matter_are_not_verse_text() {
    let i = web();
    // The preface file is not a book and gives no warning.
    assert!(i.warnings.is_empty(), "{:?}", i.warnings);
    assert!(!verse(&i, 43, 1, 51).contains("Preface"));
}

#[test]
fn verse_numbers_are_dense_and_nonempty_in_fixture() {
    let i = web();
    for b in [8usize, 43, 64] {
        for (c, verses) in i.text.books[b - 1].iter().enumerate() {
            assert!(!verses.is_empty());
            for (v, text) in verses.iter().enumerate() {
                assert!(!text.is_empty(), "book {b} {}:{}", c + 1, v + 1);
                assert_eq!(text.trim(), text);
                assert!(!text.contains("  "));
            }
        }
    }
}

const SYNTH: &str = "\\id JUD Synthetic Test (SYN)\n\\h Jude\n\\toc2 Jud\n\\c 1\n\\s1 Greeting\n\\p\n\\v 1-2 Jude, \\add a servant\\add*,\\f + \\fr 1:1 \\ft note\\f* to those\n\\q1\n\\v 3 Beloved,\\x - \\xo 1:3 \\xt Rom 1:1\\x* I was\n\\q2 diligent \\w to|strong=\"G1\" x=\"y\"\\w* write.\n\\p\n\\v 5a Part one\n\\v 5b part two\n\\v 6 \\qt-s |sid=\"q1\" who=\"Jesus\"\\*\\wj Said\\wj*\\qt-e |eid=\"q1\"\\*.\n";

#[test]
fn ranges_split_verses_and_paragraph_markers() {
    let dir = folder("synth", &[("66-JUD.SFM", SYNTH)]);
    let i = import_bible(BibleFormat::Usfm, &dir).unwrap();
    let jude = &i.text.books[64][0];
    assert_eq!(jude[0], "Jude, a servant, to those");
    assert_eq!(jude[1], jude[0]); // verse 2 shares the range text
    assert_eq!(jude[2], "Beloved, I was diligent to write.");
    assert_eq!(jude[3], "");
    assert_eq!(jude[4], "Part one part two");
    assert_eq!(jude[5], "Said.");
    assert_eq!(jude.len(), 6);
    // No "(...)" in the id line beyond the abbreviation.
    assert_eq!(i.translation.abbreviation, "SYN");
    assert_eq!(i.translation.name, "Synthetic Test");
    assert_eq!(i.translation.licence, Licence::Licensed { notice: None });
    fs::remove_dir_all(dir).ok();
}

#[test]
fn unknown_book_and_empty_folder() {
    let dir = folder(
        "unknown",
        &[("a.usfm", "\\id XYZ Nonsense\n\\c 1\n\\v 1 hi\n")],
    );
    assert!(matches!(
        import_bible(BibleFormat::Usfm, &dir),
        Err(ImportError::WrongFormat)
    ));
    fs::remove_dir_all(&dir).ok();

    let dir = folder("empty", &[("notes.txt", "hello")]);
    assert!(matches!(
        import_bible(BibleFormat::Usfm, &dir),
        Err(ImportError::WrongFormat)
    ));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn unsupported_book_is_warned_and_others_import() {
    let dir = folder(
        "mixed",
        &[
            ("a.usfm", "\\id TOB Tobit\n\\c 1\n\\v 1 hi\n"),
            ("b.usfm", "\\id RUT Test (T)\n\\c 1\n\\p\n\\v 1 Hello\n"),
        ],
    );
    let i = import_bible(BibleFormat::Usfm, &dir).unwrap();
    assert_eq!(i.text.books[7][0][0], "Hello");
    assert_eq!(i.warnings.len(), 1);
    assert!(i.warnings[0].contains("TOB"));
    fs::remove_dir_all(dir).ok();
}

#[test]
fn unterminated_footnote_warns() {
    let dir = folder(
        "badnote",
        &[(
            "a.usfm",
            "\\id RUT Test (T)\n\\c 1\n\\v 1 Hello \\f + oops\n",
        )],
    );
    let i = import_bible(BibleFormat::Usfm, &dir).unwrap();
    assert_eq!(i.text.books[7][0][0], "Hello");
    assert!(i.warnings.iter().any(|w| w.contains("unterminated")));
    fs::remove_dir_all(dir).ok();
}
