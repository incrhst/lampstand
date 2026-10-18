use std::path::PathBuf;

use lampstand_core::scripture::Licence;
use lampstand_import::bibles::{osis, zefania};
use lampstand_import::{BibleImport, ImportError};

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel)
}

fn verse(b: &BibleImport, book: usize, ch: usize, v: usize) -> &str {
    &b.text.books[book - 1][ch - 1][v - 1]
}

const GEN_1_1: &str = "In the beginning God created the heaven and the earth.";
const GEN_1_2: &str = "And the earth was without form, and void; and darkness was upon the face of the deep. And the Spirit of God moved upon the face of the waters.";
const GEN_1_3: &str = "And God said, Let there be light: and there was light.";
const JOHN_3_16: &str = "For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life.";

fn check_common(b: &BibleImport) {
    assert_eq!(b.text.books.len(), 66);
    assert_eq!(b.text.books[0][0].len(), 3);
    assert_eq!(verse(b, 1, 1, 1), GEN_1_1);
    assert_eq!(verse(b, 1, 1, 2), GEN_1_2);
    assert_eq!(verse(b, 1, 1, 3), GEN_1_3);
    assert_eq!(verse(b, 43, 3, 16), JOHN_3_16);
    assert!(verse(b, 43, 3, 17).ends_with("might be saved."));
    // Books the file lacks are empty.
    assert!(b.text.books[1].is_empty());
    assert!(b.text.books[65].is_empty());
    assert_eq!(b.translation.language, "en");
}

#[test]
fn osis_container_verses() {
    let b = osis::import(&fixture("osis/kjv-container.osis.xml")).unwrap();
    check_common(&b);
    assert_eq!(
        b.translation.name,
        "King James Version (1769) with Strongs Numbers"
    );
    assert_eq!(b.translation.abbreviation, "KJV");
    assert_eq!(b.translation.licence, Licence::PublicDomain);
    // Psalm title dropped; divine name and entities kept.
    assert_eq!(b.text.books[18][22].len(), 3);
    assert_eq!(
        verse(&b, 19, 23, 1),
        "The LORD is my shepherd; I shall not want."
    );
    assert!(verse(&b, 19, 23, 3).ends_with("for his name's sake."));
    assert!(b.warnings.is_empty(), "{:?}", b.warnings);
}

#[test]
fn osis_milestone_verses() {
    // Also covers poetry lines (<l>) running together without spaces.
    let b = osis::import(&fixture("osis/kjv-milestone.osis.xml")).unwrap();
    check_common(&b);
    assert_eq!(b.translation.abbreviation, "KJVms");
    // No rights declared: licensed, with a warning.
    assert_eq!(b.translation.licence, Licence::Licensed { notice: None });
    assert!(b.warnings.iter().any(|w| w.contains("Tob")));
    assert!(b.warnings.iter().any(|w| w.contains("licence")));
}

#[test]
fn zefania_import() {
    let b = zefania::import(&fixture("zefania/kjv.zefania.xml")).unwrap();
    check_common(&b);
    assert_eq!(b.translation.name, "King James Version");
    assert_eq!(b.translation.abbreviation, "KJV");
    assert_eq!(b.translation.licence, Licence::PublicDomain);
    assert!(b.warnings.iter().any(|w| w.contains("Tobit")));
}

#[test]
fn wrong_format_is_detected() {
    assert!(matches!(
        osis::import(&fixture("zefania/kjv.zefania.xml")),
        Err(ImportError::WrongFormat)
    ));
    assert!(matches!(
        zefania::import(&fixture("osis/kjv-container.osis.xml")),
        Err(ImportError::WrongFormat)
    ));
}

#[test]
fn missing_file_is_io_error() {
    assert!(matches!(
        osis::import(&fixture("nope.xml")),
        Err(ImportError::Io(_))
    ));
}
