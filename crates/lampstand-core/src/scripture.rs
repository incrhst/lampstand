//! Scripture: references, passages and imported translations.
//!
//! PRD: BI-1, BI-2. Phase 1 covers the 66-book Protestant canon in English
//! book names; other canons and book-name languages extend `Book` later.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::id::{BlobHash, Id};

/// A book of the Bible, 1 (Genesis) to 66 (Revelation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
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
    let input = input.trim();
    if input.is_empty() {
        return Err(ReferenceError::Empty);
    }
    // The book is everything before the first digit after the first
    // character (a leading digit is a "1 John" style prefix).
    let split = input
        .char_indices()
        .skip(1)
        .find(|(_, c)| c.is_ascii_digit())
        .map_or(input.len(), |(i, _)| i);
    let (book_text, numbers) = input.split_at(split);
    let words: Vec<&str> = book_text.split_whitespace().collect();

    // Longest run of words that names a book; leftover words are junk.
    let mut found = None;
    for n in (1..=words.len()).rev() {
        match lookup_book(&book_key(&words[..n])) {
            Ok(book) => {
                found = Some((book, n));
                break;
            }
            Err(ReferenceError::UnknownBook(_)) => {}
            Err(other) => return Err(other),
        }
    }
    let Some((book, used)) = found else {
        return Err(ReferenceError::UnknownBook(book_text.trim().to_string()));
    };
    if used < words.len() {
        return Err(ReferenceError::Malformed(input.to_string()));
    }

    let (start, end) =
        parse_numbers(numbers, book).ok_or_else(|| ReferenceError::Malformed(input.to_string()))?;
    if end < start {
        return Err(ReferenceError::Backwards);
    }
    Ok(Passage { book, start, end })
}

/// Alias keys (lowercase, letters and digits only) beyond the full names.
const ALIASES: &[(&str, u8)] = &[
    ("gen", 1),
    ("exod", 2),
    ("ex", 2),
    ("lev", 3),
    ("num", 4),
    ("deut", 5),
    ("dt", 5),
    ("josh", 6),
    ("judg", 7),
    ("jdg", 7),
    ("1sam", 9),
    ("1sm", 9),
    ("2sam", 10),
    ("2sm", 10),
    ("1kgs", 11),
    ("1ki", 11),
    ("2kgs", 12),
    ("2ki", 12),
    ("1chr", 13),
    ("1ch", 13),
    ("2chr", 14),
    ("2ch", 14),
    ("neh", 16),
    ("esth", 17),
    ("ps", 19),
    ("pss", 19),
    ("psalm", 19),
    ("pslm", 19),
    ("prov", 20),
    ("prv", 20),
    ("pr", 20),
    ("eccl", 21),
    ("ecc", 21),
    ("qoh", 21),
    ("song", 22),
    ("songofsongs", 22),
    ("songofsolomon", 22),
    ("sos", 22),
    ("cant", 22),
    ("canticles", 22),
    ("isa", 23),
    ("jer", 24),
    ("lam", 25),
    ("ezek", 26),
    ("eze", 26),
    ("dan", 27),
    ("hos", 28),
    ("obad", 31),
    ("ob", 31),
    ("jon", 32),
    ("mic", 33),
    ("nah", 34),
    ("hab", 35),
    ("zeph", 36),
    ("hag", 37),
    ("zech", 38),
    ("mal", 39),
    ("matt", 40),
    ("mt", 40),
    ("mk", 41),
    ("mrk", 41),
    ("lk", 42),
    ("luk", 42),
    ("jn", 43),
    ("jhn", 43),
    ("rom", 45),
    ("1cor", 46),
    ("2cor", 47),
    ("gal", 48),
    ("eph", 49),
    ("phil", 50),
    ("php", 50),
    ("col", 51),
    ("1thess", 52),
    ("1thes", 52),
    ("1th", 52),
    ("2thess", 53),
    ("2thes", 53),
    ("2th", 53),
    ("1tim", 54),
    ("1ti", 54),
    ("2tim", 55),
    ("2ti", 55),
    ("tit", 56),
    ("phlm", 57),
    ("phm", 57),
    ("heb", 58),
    ("jas", 59),
    ("jm", 59),
    ("1pet", 60),
    ("1pt", 60),
    ("2pet", 61),
    ("2pt", 61),
    ("1jn", 62),
    ("2jn", 63),
    ("3jn", 64),
    ("rev", 66),
    ("rv", 66),
];

/// Lowercase letters and digits only: "1 Cor." becomes "1cor".
fn squash(text: &str) -> String {
    text.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// The lookup key for the words of a book name, turning a leading "I",
/// "II", "III", "First", "1st" and so on into a digit.
fn book_key(words: &[&str]) -> String {
    if let Some((first, rest)) = words.split_first() {
        let digit = match squash(first).as_str() {
            "i" | "1st" | "first" => Some('1'),
            "ii" | "2nd" | "second" => Some('2'),
            "iii" | "3rd" | "third" => Some('3'),
            _ => None,
        };
        if let (Some(digit), false) = (digit, rest.is_empty()) {
            return format!("{digit}{}", squash(&rest.join(" ")));
        }
    }
    squash(&words.join(" "))
}

/// Exact full name or alias first, then a prefix of any name or alias that
/// points at exactly one book.
fn lookup_book(key: &str) -> Result<Book, ReferenceError> {
    let unknown = || ReferenceError::UnknownBook(key.to_string());
    if key.is_empty() {
        return Err(unknown());
    }
    let names = || {
        BOOK_NAMES
            .iter()
            .zip(1u8..)
            .map(|(name, number)| (squash(name), number))
            .chain(
                ALIASES
                    .iter()
                    .map(|&(alias, number)| (alias.to_string(), number)),
            )
    };
    if let Some((_, number)) = names().find(|(name, _)| name == key) {
        return Ok(Book(number));
    }
    let mut matches: Vec<Book> = names()
        .filter(|(name, _)| name.starts_with(key))
        .map(|(_, number)| Book(number))
        .collect();
    matches.sort();
    matches.dedup();
    match matches.as_slice() {
        [] => Err(unknown()),
        [only] => Ok(*only),
        _ => Err(ReferenceError::AmbiguousBook(key.to_string(), matches)),
    }
}

/// A number that fits a chapter or verse (never zero or `END_OF_CHAPTER`).
fn parse_count(text: &str) -> Option<u16> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse::<u16>()
        .ok()
        .filter(|n| (1..ChapterVerse::END_OF_CHAPTER).contains(n))
}

/// "16" or "3:16" as (first, optional second).
fn parse_point(text: &str) -> Option<(u16, Option<u16>)> {
    match text.split_once(':') {
        None => Some((parse_count(text)?, None)),
        Some((a, b)) => Some((parse_count(a)?, Some(parse_count(b)?))),
    }
}

/// The chapter and verse part of a reference, after the book.
fn parse_numbers(text: &str, book: Book) -> Option<(ChapterVerse, ChapterVerse)> {
    let cleaned: String = text
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| match c {
            '\u{2013}' | '\u{2014}' | '\u{2212}' => '-',
            '.' => ':',
            c => c,
        })
        .collect();
    let cv = |chapter, verse| ChapterVerse { chapter, verse };
    let end_of = ChapterVerse::END_OF_CHAPTER;
    if cleaned.is_empty() {
        // A bare "Jude" is the whole book.
        return book.is_single_chapter().then(|| (cv(1, 1), cv(1, end_of)));
    }
    let (first, second) = match cleaned.split_once('-') {
        Some((a, b)) => (parse_point(a)?, Some(parse_point(b)?)),
        None => (parse_point(&cleaned)?, None),
    };
    match (first, second) {
        // "Jude 3", "Jude 3-5": verses of a one-chapter book.
        ((verse, None), second) if book.is_single_chapter() => {
            let end = match second {
                None => cv(1, verse),
                Some((v, None)) => cv(1, v),
                Some((c, Some(v))) => cv(c, v),
            };
            Some((cv(1, verse), end))
        }
        // "Psalm 23", "Romans 8-9": whole chapters.
        ((chapter, None), None) => Some((cv(chapter, 1), cv(chapter, end_of))),
        ((chapter, None), Some((last, None))) => Some((cv(chapter, 1), cv(last, end_of))),
        ((_, None), Some((_, Some(_)))) => None,
        // "John 3:16", "3:16-18", "1:1-2:3".
        ((chapter, Some(verse)), None) => Some((cv(chapter, verse), cv(chapter, verse))),
        ((chapter, Some(verse)), Some((last, None))) => {
            Some((cv(chapter, verse), cv(chapter, last)))
        }
        ((chapter, Some(verse)), Some((last_chapter, Some(last_verse)))) => {
            Some((cv(chapter, verse), cv(last_chapter, last_verse)))
        }
    }
}

impl fmt::Display for Passage {
    /// "John 3:16", "John 3:16–18", "Genesis 1:1–2:3", "Psalms 23", "Jude 3".
    /// An end that is `END_OF_CHAPTER` but follows a start past verse 1
    /// prints as "end" ("John 3:16–end"), which does not parse back.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const END: u16 = ChapterVerse::END_OF_CHAPTER;
        let (s, e) = (self.start, self.end);
        let name = self.book.name();
        let verse = |v: u16| {
            if v == END {
                "end".to_string()
            } else {
                v.to_string()
            }
        };
        if self.book.is_single_chapter() && s.chapter == 1 && e.chapter == 1 {
            return match (s.verse, e.verse) {
                (1, END) => f.write_str(name),
                (a, b) if a == b => write!(f, "{name} {a}"),
                (a, b) => write!(f, "{name} {a}–{}", verse(b)),
            };
        }
        if s.chapter == e.chapter {
            return match (s.verse, e.verse) {
                (1, END) => write!(f, "{name} {}", s.chapter),
                (a, b) if a == b => write!(f, "{name} {}:{a}", s.chapter),
                (a, b) => write!(f, "{name} {}:{a}–{}", s.chapter, verse(b)),
            };
        }
        if s.verse == 1 && e.verse == END {
            write!(f, "{name} {}–{}", s.chapter, e.chapter)
        } else {
            write!(
                f,
                "{name} {}:{}–{}:{}",
                s.chapter,
                s.verse,
                e.chapter,
                verse(e.verse)
            )
        }
    }
}

pub type TranslationId = Id<Translation>;

/// An imported Bible translation. The metadata is a synced document; the
/// text is a blob ([`BibleText`], postcard + zstd) that syncs between the
/// church's own devices and is never put in a service package (BI-2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
        let chapters = self
            .books
            .get(usize::from(passage.book.number()) - 1)
            .filter(|chapters| !chapters.is_empty())
            .ok_or(LookupError::NotInTranslation)?;
        let (start, end) = (passage.start, passage.end);
        let first = usize::from(start.chapter).saturating_sub(1);
        let first_verse = usize::from(start.verse).saturating_sub(1);
        if chapters.get(first).is_none_or(|c| first_verse >= c.len()) {
            return Err(LookupError::NotInTranslation);
        }
        let last = usize::from(end.chapter)
            .saturating_sub(1)
            .min(chapters.len() - 1);
        let mut out = Vec::new();
        for (index, verses) in chapters.iter().enumerate().take(last + 1).skip(first) {
            let from = if index == first { first_verse } else { 0 };
            let to = if index == last {
                usize::from(end.verse).min(verses.len())
            } else {
                verses.len()
            };
            for (v, text) in verses.iter().enumerate().take(to).skip(from) {
                let at = ChapterVerse {
                    chapter: index as u16 + 1,
                    verse: v as u16 + 1,
                };
                out.push((at, text.as_str()));
            }
        }
        Ok(out)
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
    fn displays_passages() {
        assert_eq!(passage(43, (3, 16), (3, 16)).to_string(), "John 3:16");
        assert_eq!(passage(43, (3, 16), (3, 18)).to_string(), "John 3:16–18");
        assert_eq!(passage(1, (1, 1), (2, 3)).to_string(), "Genesis 1:1–2:3");
        assert_eq!(passage(19, (23, 1), (23, END)).to_string(), "Psalms 23");
        assert_eq!(passage(65, (1, 3), (1, 3)).to_string(), "Jude 3");
    }

    #[test]
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

    #[test]
    fn round_trips_through_display() {
        let inputs = [
            "John 3:16",
            "Jn 3:16-18",
            "1 Cor 13:4–7",
            "First Corinthians 13",
            "Psalm 23",
            "Gen 1:1-2:3",
            "Romans 8-9",
            "Jude 3",
            "Jude 3-5",
            "Jude",
            "Phm 6",
            "Song of Songs 2:4",
            "Rev 22:21",
        ];
        for input in inputs {
            let first = parse_reference(input).unwrap();
            let shown = first.to_string();
            assert_eq!(parse_reference(&shown), Ok(first), "{input} -> {shown}");
        }
    }

    #[test]
    fn every_book_round_trips_by_name() {
        for number in 1..=66 {
            let book = Book::new(number).unwrap();
            for p in [
                passage(number, (1, 1), (1, END)),
                passage(number, (1, 2), (1, 5)),
                passage(number, (1, 3), (1, 3)),
            ] {
                let shown = p.to_string();
                assert_eq!(parse_reference(&shown), Ok(p), "{} {shown}", book.name());
            }
        }
    }
}
