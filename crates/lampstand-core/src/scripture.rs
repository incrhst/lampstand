//! Scripture: references, passages and imported translations.
//!
//! PRD: BI-1, BI-2. Phase 1 covers the 66-book Protestant canon in English
//! book names; other canons and book-name languages extend `Book` later.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::id::{BlobHash, Id};

/// A book of the Bible, 1 (Genesis) to 66 (Revelation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(
    feature = "automerge",
    derive(autosurgeon::Hydrate, autosurgeon::Reconcile)
)]
pub struct Book(u8);

pub const BOOK_NAMES: [&str; 66] = [
    "Genesis",
    "Exodus",
    "Leviticus",
    "Numbers",
    "Deuteronomy",
    "Joshua",
    "Judges",
    "Ruth",
    "1 Samuel",
    "2 Samuel",
    "1 Kings",
    "2 Kings",
    "1 Chronicles",
    "2 Chronicles",
    "Ezra",
    "Nehemiah",
    "Esther",
    "Job",
    "Psalms",
    "Proverbs",
    "Ecclesiastes",
    "Song of Solomon",
    "Isaiah",
    "Jeremiah",
    "Lamentations",
    "Ezekiel",
    "Daniel",
    "Hosea",
    "Joel",
    "Amos",
    "Obadiah",
    "Jonah",
    "Micah",
    "Nahum",
    "Habakkuk",
    "Zephaniah",
    "Haggai",
    "Zechariah",
    "Malachi",
    "Matthew",
    "Mark",
    "Luke",
    "John",
    "Acts",
    "Romans",
    "1 Corinthians",
    "2 Corinthians",
    "Galatians",
    "Ephesians",
    "Philippians",
    "Colossians",
    "1 Thessalonians",
    "2 Thessalonians",
    "1 Timothy",
    "2 Timothy",
    "Titus",
    "Philemon",
    "Hebrews",
    "James",
    "1 Peter",
    "2 Peter",
    "1 John",
    "2 John",
    "3 John",
    "Jude",
    "Revelation",
];

impl Book {
    pub fn new(number: u8) -> Option<Self> {
        (1..=66).contains(&number).then_some(Self(number))
    }

    pub fn number(self) -> u8 {
        self.0
    }

    pub fn name(self) -> &'static str {
        BOOK_NAMES[usize::from(self.0) - 1]
    }

    /// Obadiah, Philemon, 2 John, 3 John and Jude, where "Jude 3" means 1:3.
    pub fn is_single_chapter(self) -> bool {
        matches!(self.0, 31 | 57 | 63 | 64 | 65)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(
    feature = "automerge",
    derive(autosurgeon::Hydrate, autosurgeon::Reconcile)
)]
pub struct ChapterVerse {
    pub chapter: u16,
    pub verse: u16,
}

impl ChapterVerse {
    /// As an end verse: the last verse of the chapter, whatever it is in
    /// the translation used ("Psalm 23" ends at 23:END_OF_CHAPTER).
    pub const END_OF_CHAPTER: u16 = u16::MAX;
}

/// A contiguous passage in one book, inclusive at both ends. It may cross
/// chapters ("Gen 1:1-2:3").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "automerge",
    derive(autosurgeon::Hydrate, autosurgeon::Reconcile)
)]
pub struct Passage {
    pub book: Book,
    pub start: ChapterVerse,
    pub end: ChapterVerse,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReferenceError {
    Empty,
    UnknownBook(String),
    /// The abbreviation matches more than one book ("J").
    AmbiguousBook(String, Vec<Book>),
    Malformed(String),
    /// The end comes before the start ("John 3:18-16").
    Backwards,
}

/// Parses what an operator types: "Jn 3:16-18", "1 Cor 13:4–7", "Psalm 23",
/// "Gen 1:1-2:3", "Jude 3", "John 3.16". Book names match full names, the
/// SBL abbreviations and unambiguous prefixes, ignoring case, spaces and a
/// trailing full stop; "I", "II", "III" and "First" stand for 1, 2, 3.
/// Hyphen, en dash and em dash all work as range separators.
pub fn parse_reference(input: &str) -> Result<Passage, ReferenceError> {
    let _ = input;
    todo!("T01")
}

impl fmt::Display for Passage {
    /// "John 3:16", "John 3:16–18", "Genesis 1:1–2:3", "Psalms 23", "Jude 3".
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let _ = f;
        todo!("T01")
    }
}

pub type TranslationId = Id<Translation>;

/// An imported Bible translation. The metadata is a synced document; the
/// text is a blob ([`BibleText`], postcard + zstd) that syncs between the
/// church's own devices and is never put in a service package (BI-2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "automerge",
    derive(autosurgeon::Hydrate, autosurgeon::Reconcile)
)]
pub struct Translation {
    pub id: TranslationId,
    pub name: String,
    /// Shown on slides: "KJV", "NIV".
    pub abbreviation: String,
    /// BCP 47 tag: "en", "es-419".
    pub language: String,
    pub licence: Licence,
    pub text: BlobHash,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "automerge",
    derive(autosurgeon::Hydrate, autosurgeon::Reconcile)
)]
pub enum Licence {
    PublicDomain,
    /// Imported by the church under its own licence; the notice is shown
    /// with the text where the publisher requires it.
    Licensed {
        notice: Option<String>,
    },
}

/// The full text of a translation. `books[b][c][v]` is book b+1, chapter
/// c+1, verse v+1, using the translation's own versification. A book the
/// translation lacks is an empty Vec.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct BibleText {
    pub books: Vec<Vec<Vec<String>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LookupError {
    /// The passage starts beyond the text the translation has.
    NotInTranslation,
}

impl BibleText {
    /// The verses of a passage, clamping an end beyond the chapter (or
    /// `END_OF_CHAPTER`) to the chapter's last verse.
    pub fn verses(&self, passage: &Passage) -> Result<Vec<(ChapterVerse, &str)>, LookupError> {
        let _ = passage;
        todo!("T01")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn passage(book: u8, (c1, v1): (u16, u16), (c2, v2): (u16, u16)) -> Passage {
        Passage {
            book: Book::new(book).unwrap(),
            start: ChapterVerse {
                chapter: c1,
                verse: v1,
            },
            end: ChapterVerse {
                chapter: c2,
                verse: v2,
            },
        }
    }

    const END: u16 = ChapterVerse::END_OF_CHAPTER;

    #[test]
    fn book_names_line_up() {
        assert_eq!(Book::new(43).unwrap().name(), "John");
        assert_eq!(Book::new(66).unwrap().name(), "Revelation");
        assert!(Book::new(0).is_none() && Book::new(67).is_none());
        assert!(Book::new(65).unwrap().is_single_chapter());
    }

    #[test]
    #[ignore = "T01"]
    fn parses_common_references() {
        let cases = [
            ("Jn 3:16-18", passage(43, (3, 16), (3, 18))),
            ("john 3:16", passage(43, (3, 16), (3, 16))),
            ("John 3.16", passage(43, (3, 16), (3, 16))),
            ("1 Cor 13:4-7", passage(46, (13, 4), (13, 7))),
            ("1Co 13:4–7", passage(46, (13, 4), (13, 7))),
            ("I Corinthians 13:4—7", passage(46, (13, 4), (13, 7))),
            ("First Corinthians 13", passage(46, (13, 1), (13, END))),
            ("Psalm 23", passage(19, (23, 1), (23, END))),
            ("Ps. 119:105", passage(19, (119, 105), (119, 105))),
            ("Gen 1:1-2:3", passage(1, (1, 1), (2, 3))),
            ("Romans 8-9", passage(45, (8, 1), (9, END))),
            ("Jude 3", passage(65, (1, 3), (1, 3))),
            ("Jude 3-5", passage(65, (1, 3), (1, 5))),
            ("Phil 4:13", passage(50, (4, 13), (4, 13))),
            ("Phm 6", passage(57, (1, 6), (1, 6))),
            ("Song of Songs 2:4", passage(22, (2, 4), (2, 4))),
            ("  rev 22:21 ", passage(66, (22, 21), (22, 21))),
        ];
        for (input, expected) in cases {
            assert_eq!(parse_reference(input), Ok(expected), "{input}");
        }
    }

    #[test]
    #[ignore = "T01"]
    fn rejects_bad_references() {
        assert_eq!(parse_reference(""), Err(ReferenceError::Empty));
        assert!(matches!(
            parse_reference("Hezekiah 1:1"),
            Err(ReferenceError::UnknownBook(_))
        ));
        assert!(matches!(
            parse_reference("J 3:16"),
            Err(ReferenceError::AmbiguousBook(..))
        ));
        assert_eq!(
            parse_reference("John 3:18-16"),
            Err(ReferenceError::Backwards)
        );
        assert!(matches!(
            parse_reference("John three"),
            Err(ReferenceError::Malformed(_))
        ));
    }

    #[test]
    #[ignore = "T01"]
    fn displays_passages() {
        assert_eq!(passage(43, (3, 16), (3, 16)).to_string(), "John 3:16");
        assert_eq!(passage(43, (3, 16), (3, 18)).to_string(), "John 3:16–18");
        assert_eq!(passage(1, (1, 1), (2, 3)).to_string(), "Genesis 1:1–2:3");
        assert_eq!(passage(19, (23, 1), (23, END)).to_string(), "Psalms 23");
        assert_eq!(passage(65, (1, 3), (1, 3)).to_string(), "Jude 3");
    }

    #[test]
    #[ignore = "T01"]
    fn looks_up_verses() {
        let mut books = vec![Vec::new(); 66];
        books[42] = vec![
            vec!["one".to_string(), "two".to_string()],
            vec!["three".to_string(), "four".to_string(), "five".to_string()],
        ];
        let text = BibleText { books };
        let got = text.verses(&passage(43, (1, 2), (2, END))).unwrap();
        let words: Vec<&str> = got.iter().map(|(_, t)| *t).collect();
        assert_eq!(words, ["two", "three", "four", "five"]);
        assert_eq!(
            got[0].0,
            ChapterVerse {
                chapter: 1,
                verse: 2
            }
        );
        assert_eq!(
            text.verses(&passage(43, (9, 1), (9, 1))),
            Err(LookupError::NotInTranslation)
        );
    }
}
