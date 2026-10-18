use std::path::{Path, PathBuf};

use lampstand_core::scripture::Licence;
use lampstand_import::bibles::{openlp, opensong};
use lampstand_import::{import_bible, BibleFormat, ImportError};

fn fixture(dir: &str, file: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(dir)
        .join(file)
}

fn verse(books: &[Vec<Vec<String>>], book: usize, c: usize, v: usize) -> &str {
    &books[book - 1][c - 1][v - 1]
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lampstand-t13-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn build_openlp() -> PathBuf {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = temp(&format!("db{n}"));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("kjv_sample.sqlite");
    let _ = std::fs::remove_file(&path);
    let sql = std::fs::read_to_string(fixture("openlp_bible", "kjv_sample.sql")).unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch(&sql)
        .unwrap();
    path
}

#[test]
fn opensong_reads_books_chapters_verses() {
    let imp = opensong::import(&fixture("opensong_bible", "kjv_sample.xml")).unwrap();
    let b = &imp.text.books;
    assert_eq!(b.len(), 66);
    assert_eq!(
        verse(b, 1, 1, 1),
        "In the beginning God created the heaven and the earth."
    );
    // Hand-wrapped text is collapsed to single spaces.
    assert!(verse(b, 1, 1, 2).ends_with("darkness was upon the face of the deep. And the Spirit of God moved upon the face of the waters."));
    assert_eq!(b[18][22].len(), 3); // Psalm 23, via "Ps."
    assert!(verse(b, 43, 3, 16).starts_with("For God so loved"));
    assert!(verse(b, 62, 1, 1).starts_with("That which was from")); // "I John"
                                                                    // Chapters before the first present one are empty placeholders.
    assert!(b[42][0].is_empty());
    assert!(b[1].is_empty());
}

#[test]
fn opensong_metadata_and_warnings() {
    let imp = opensong::import(&fixture("opensong_bible", "kjv_sample.xml")).unwrap();
    assert_eq!(imp.translation.name, "King James Version (sample)");
    assert_eq!(imp.translation.abbreviation, "kjv_sample");
    assert!(matches!(imp.translation.licence, Licence::Licensed { .. }));
    assert_eq!(imp.warnings.len(), 1);
    assert!(imp.warnings[0].contains("Apocrypha Thing"));
}

#[test]
fn opensong_decodes_entities_and_ranges() {
    let path = temp("entities.xml");
    std::fs::write(
        &path,
        r#"<bible><b n="Jude"><c n="1"><v n="1-2">Jude &amp; &lt;servant&gt; &#8212; ok</v><v n="3"><![CDATA[a < b]]></v></c></b></bible>"#,
    )
    .unwrap();
    let imp = opensong::import(&path).unwrap();
    assert_eq!(verse(&imp.text.books, 65, 1, 1), "Jude & <servant> — ok");
    assert_eq!(verse(&imp.text.books, 65, 1, 2), "");
    assert_eq!(verse(&imp.text.books, 65, 1, 3), "a < b");
    assert_eq!(imp.translation.name, "entities"); // no name attribute
}

#[test]
fn opensong_rejects_other_files() {
    let path = temp("not.xml");
    std::fs::write(&path, "<osis><x/></osis>").unwrap();
    assert!(matches!(
        opensong::import(&path),
        Err(ImportError::WrongFormat)
    ));
    std::fs::write(&path, "plain text").unwrap();
    assert!(matches!(
        opensong::import(&path),
        Err(ImportError::WrongFormat)
    ));
    assert!(matches!(
        opensong::import(Path::new("/nonexistent/x.xml")),
        Err(ImportError::Io(_)) | Err(ImportError::Unreadable(_))
    ));
}

#[test]
fn opensong_book_names() {
    use lampstand_core::scripture::BOOK_NAMES;
    // Every canonical name maps to itself.
    for (i, name) in BOOK_NAMES.iter().enumerate() {
        assert_eq!(opensong_book(name), Some(i as u8 + 1), "{name}");
    }
    for (name, n) in [
        ("Gen", 1),
        ("Psalm", 19),
        ("Song of Songs", 22),
        ("Canticles", 22),
        ("I Samuel", 9),
        ("II Kings", 12),
        ("First Chronicles", 13),
        ("2nd Corinthians", 47),
        ("1Thess.", 52),
        ("III John", 64),
        ("Phil", 50),
        ("Philemon", 57),
        ("Jude", 65),
        ("Rev.", 66),
        ("Revelation of John", 66),
        ("Deut", 5),
    ] {
        assert_eq!(opensong_book(name), Some(n), "{name}");
    }
    for bad in ["", "Tobit", "Foo", "J"] {
        assert_eq!(opensong_book(bad), None, "{bad}");
    }
}

fn opensong_book(name: &str) -> Option<u8> {
    let path = temp("names.xml");
    std::fs::write(
        &path,
        format!(r#"<bible><b n="{name}"><c n="1"><v n="1">x</v></c></b></bible>"#),
    )
    .unwrap();
    match opensong::import(&path) {
        Ok(imp) => imp
            .text
            .books
            .iter()
            .position(|b| !b.is_empty())
            .map(|i| i as u8 + 1),
        Err(_) => None,
    }
}

#[test]
fn openlp_reads_books_by_reference_id_and_name() {
    let imp = openlp::import(&build_openlp()).unwrap();
    let b = &imp.text.books;
    assert_eq!(b.len(), 66);
    assert_eq!(
        verse(b, 19, 23, 1),
        "The LORD is my shepherd; I shall not want."
    );
    assert_eq!(b[18][22].len(), 2);
    assert_eq!(
        verse(b, 1, 1, 1),
        "In the beginning God created the heaven and the earth."
    );
    assert!(verse(b, 1, 1, 2).contains("deep. And the Spirit"));
    assert!(verse(b, 62, 1, 1).starts_with("That which was from"));
    // book_reference_id 0 falls back to the name "John".
    assert!(verse(b, 43, 3, 16).starts_with("For God so loved"));
    assert!(imp.warnings.is_empty());
}

#[test]
fn openlp_metadata() {
    let imp = openlp::import(&build_openlp()).unwrap();
    assert_eq!(imp.translation.name, "King James Version (sample)");
    assert_eq!(imp.translation.abbreviation, "kjv_sample");
}

#[test]
fn openlp_rejects_other_files() {
    let path = temp("not.sqlite");
    std::fs::write(&path, "this is not a database at all, just text").unwrap();
    assert!(matches!(
        openlp::import(&path),
        Err(ImportError::WrongFormat)
    ));

    // A valid database without the Bible tables.
    let other = temp("other.sqlite");
    let _ = std::fs::remove_file(&other);
    rusqlite::Connection::open(&other)
        .unwrap()
        .execute_batch("CREATE TABLE songs (id INTEGER);")
        .unwrap();
    assert!(matches!(
        openlp::import(&other),
        Err(ImportError::WrongFormat)
    ));
    assert!(matches!(
        openlp::import(Path::new("/nonexistent/x.sqlite")),
        Err(ImportError::Io(_))
    ));
}

#[test]
fn dispatch_through_import_bible() {
    let a = import_bible(
        BibleFormat::OpenSong,
        &fixture("opensong_bible", "kjv_sample.xml"),
    )
    .unwrap();
    let b = import_bible(BibleFormat::OpenLp, &build_openlp()).unwrap();
    // Same text, whichever format it came from.
    assert_eq!(a.text.books[0][0][0], b.text.books[0][0][0]);
    assert_eq!(a.text.books[42][2][15], b.text.books[42][2][15]);
}
