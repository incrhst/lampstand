//! The same public-domain KJV excerpt (Gen 1:1-5, 2:1-3; Ps 23; John
//! 3:16-18) in each supported format must import to identical text.

use lampstand_scripture::import::{self, Format, Import};
use lampstand_scripture::{split, Book, Budget, Reference, TranslationMeta};

const GEN: &str = include_str!("fixtures/GEN.usfm");
const PSA: &str = include_str!("fixtures/PSA.usfm");
const JHN: &str = include_str!("fixtures/JHN.usfm");
const OSIS: &str = include_str!("fixtures/kjv-excerpt.osis.xml");
const ZEFANIA: &str = include_str!("fixtures/kjv-excerpt.zefania.xml");
const OPENSONG: &str = include_str!("fixtures/kjv-excerpt.opensong.xml");

fn usfm() -> Import {
    let meta = TranslationMeta {
        abbreviation: "KJV".into(),
        name: "King James Version".into(),
        ..Default::default()
    };
    import::usfm::import([GEN, PSA, JHN], meta).unwrap()
}

fn all() -> Vec<Import> {
    vec![
        usfm(),
        import::import(OSIS).unwrap(),
        import::import(ZEFANIA).unwrap(),
        import::import(OPENSONG).unwrap(),
    ]
}

#[test]
fn every_format_imports_the_same_text() {
    let imports = all();
    let formats: Vec<Format> = imports.iter().map(|i| i.format).collect();
    assert_eq!(
        formats,
        [
            Format::Usfm,
            Format::Osis,
            Format::Zefania,
            Format::OpenSong
        ]
    );
    let reference = &imports[0].translation;
    assert_eq!(reference.verse_count(), 17);
    for i in &imports[1..] {
        for book in [Book::GENESIS, Book::PSALMS, Book::JOHN] {
            for chapter in 1..=23 {
                for verse in 1..=18 {
                    assert_eq!(
                        i.translation.verse(book, chapter, verse),
                        reference.verse(book, chapter, verse),
                        "{} {book} {chapter}:{verse}",
                        i.format
                    );
                }
            }
        }
        assert_eq!(i.translation.verse_count(), 17, "{}", i.format);
    }
}

#[test]
fn markup_notes_and_headings_are_removed() {
    let t = usfm().translation;
    assert_eq!(
        t.verse(Book::GENESIS, 1, 1),
        Some("In the beginning God created the heaven and the earth.")
    );
    assert_eq!(
        t.verse(Book::GENESIS, 1, 4),
        Some(
            "And God saw the light, that it was good: and God divided the light from the darkness."
        )
    );
    assert_eq!(
        t.verse(Book::PSALMS, 23, 1),
        Some("The LORD is my shepherd; I shall not want.")
    );
    assert_eq!(
        t.verse(Book::JOHN, 3, 17),
        Some("For God sent not his Son into the world to condemn the world; but that the world through him might be saved.")
    );
}

#[test]
fn metadata_comes_through() {
    let osis = import::import(OSIS).unwrap().translation.meta;
    assert_eq!(osis.abbreviation, "KJV");
    assert_eq!(osis.name, "King James Version (excerpt)");
    assert_eq!(osis.language.as_deref(), Some("en"));
    assert_eq!(
        osis.copyright.as_deref(),
        Some("Public domain outside the United Kingdom.")
    );
    assert_eq!(
        osis.licence.as_deref(),
        Some("Free to copy, project and distribute.")
    );

    let zefania = import::import(ZEFANIA).unwrap().translation.meta;
    assert_eq!(zefania.abbreviation, "KJV");
    assert_eq!(zefania.name, "King James Version (excerpt)");
    assert_eq!(
        zefania.copyright.as_deref(),
        Some("Public domain outside the United Kingdom.")
    );
}

#[test]
fn unknown_books_are_reported() {
    for source in [OSIS, ZEFANIA] {
        let report = import::import(source).unwrap().report;
        assert_eq!(report.skipped.len(), 1, "{report:?}");
        assert!(report.skipped[0].reason.contains("apocryphal"));
    }
    assert!(usfm().report.skipped.is_empty());
    assert!(import::import(OPENSONG).unwrap().report.skipped.is_empty());
}

#[test]
fn wrong_format_and_bad_xml() {
    assert_eq!(
        import::osis::import(ZEFANIA).unwrap_err(),
        import::ImportError::WrongFormat {
            expected: Format::Osis
        }
    );
    assert!(matches!(
        import::import("<osis><unclosed></osis>"),
        Err(import::ImportError::Xml(_))
    ));
}

#[test]
fn type_a_reference_and_split_it() {
    let t = usfm().translation;
    let reference: Reference = "Ps 23:1-4,6".parse().unwrap();
    let verses = t.passage(&reference).unwrap();
    assert_eq!(
        verses.iter().map(|v| v.verse).collect::<Vec<_>>(),
        [1, 2, 3, 4, 6]
    );

    let budget = Budget {
        chars_per_line: 36,
        lines: 4,
    };
    let slides = split(&verses, budget);
    assert!(slides.len() > 1);
    for slide in &slides {
        assert!(
            slide.lines(budget.chars_per_line).len() <= budget.lines,
            "{}",
            slide.text()
        );
    }
    assert!(slides[0].text().starts_with("1 The LORD is my shepherd"));

    let cross: Reference = "Gen 1:4-2:1".parse().unwrap();
    let verses = t.passage(&cross).unwrap();
    let slides = split(
        &verses,
        Budget {
            chars_per_line: 200,
            lines: 20,
        },
    );
    assert!(slides[0].text().starts_with("1:4 And God saw"));
    assert!(slides[0].text().contains(" 2:1 Thus the heavens"));

    let err = t.passage(&"Ps 23:7".parse().unwrap()).unwrap_err();
    assert_eq!(
        err.to_string(),
        "Psalms 23 has 6 verses; there is no verse 7"
    );
}
