//! An imported translation: its text and the licence the church holds it
//! under.
//!
//! PRD: BI-1, BI-2.

use std::collections::BTreeMap;
use std::fmt;

use crate::book::Book;
use crate::reference::Reference;

/// Who published a translation and on what terms. Shown to the operator on
/// import and kept with the text so a copyright line can go on screen.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TranslationMeta {
    /// Short name shown in pickers, e.g. "KJV".
    pub abbreviation: String,
    pub name: String,
    /// BCP 47 tag where the source gives one, e.g. "en".
    pub language: Option<String>,
    /// Copyright or rights statement as the source states it.
    pub copyright: Option<String>,
    /// Licence or permission text, where separate from the copyright line.
    pub licence: Option<String>,
}

/// One verse of text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verse {
    pub book: Book,
    pub chapter: u16,
    pub verse: u16,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Translation {
    pub meta: TranslationMeta,
    text: BTreeMap<Book, BTreeMap<u16, BTreeMap<u16, String>>>,
}

/// Why a parsed reference has no text in this translation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PassageError {
    MissingBook {
        book: Book,
        translation: String,
    },
    MissingChapter {
        book: Book,
        chapter: u16,
        translation: String,
    },
    MissingVerse {
        book: Book,
        chapter: u16,
        verse: u16,
        last: u16,
    },
}

impl fmt::Display for PassageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PassageError::MissingBook { book, translation } => {
                write!(f, "{translation} does not include {book}")
            }
            PassageError::MissingChapter {
                book,
                chapter,
                translation,
            } => {
                write!(f, "{translation} does not include {book} {chapter}")
            }
            PassageError::MissingVerse {
                book,
                chapter,
                verse,
                last,
            } => {
                write!(
                    f,
                    "{book} {chapter} has {last} verses; there is no verse {verse}"
                )
            }
        }
    }
}

impl std::error::Error for PassageError {}

impl Translation {
    pub fn new(meta: TranslationMeta) -> Translation {
        Translation {
            meta,
            text: BTreeMap::new(),
        }
    }

    /// Add or replace a verse, returning the text it replaced.
    pub fn insert(&mut self, book: Book, chapter: u16, verse: u16, text: String) -> Option<String> {
        self.text
            .entry(book)
            .or_default()
            .entry(chapter)
            .or_default()
            .insert(verse, text)
    }

    pub fn verse(&self, book: Book, chapter: u16, verse: u16) -> Option<&str> {
        self.text
            .get(&book)?
            .get(&chapter)?
            .get(&verse)
            .map(String::as_str)
    }

    /// Books present, in canonical order.
    pub fn books(&self) -> impl Iterator<Item = Book> + '_ {
        self.text.keys().copied()
    }

    /// Highest verse number in a chapter, if the chapter is present.
    pub fn last_verse(&self, book: Book, chapter: u16) -> Option<u16> {
        self.text
            .get(&book)?
            .get(&chapter)?
            .keys()
            .next_back()
            .copied()
    }

    pub fn verse_count(&self) -> usize {
        self.text
            .values()
            .flat_map(|c| c.values())
            .map(|v| v.len())
            .sum()
    }

    /// Every verse the reference covers, in the order typed. Fails when a
    /// chapter or verse the operator named is not in this translation, so a
    /// typo such as "Ps 23:16" is caught rather than shown as nothing.
    pub fn passage(&self, reference: &Reference) -> Result<Vec<Verse>, PassageError> {
        let book = reference.book;
        let label = || self.label();
        let chapters = self
            .text
            .get(&book)
            .ok_or_else(|| PassageError::MissingBook {
                book,
                translation: label(),
            })?;
        let check = |chapter: u16, verse: Option<u16>| -> Result<(), PassageError> {
            let verses = chapters
                .get(&chapter)
                .ok_or_else(|| PassageError::MissingChapter {
                    book,
                    chapter,
                    translation: label(),
                })?;
            match verse {
                Some(v) if !verses.contains_key(&v) => Err(PassageError::MissingVerse {
                    book,
                    chapter,
                    verse: v,
                    last: verses.keys().next_back().copied().unwrap_or(0),
                }),
                _ => Ok(()),
            }
        };
        let mut out = Vec::new();
        for range in &reference.ranges {
            check(range.start.chapter, range.start.verse)?;
            check(range.end.chapter, range.end.verse)?;
            for (&c, verses) in chapters.range(range.start.chapter..=range.end.chapter) {
                for (&v, text) in verses {
                    if range.contains(c, v) {
                        out.push(Verse {
                            book,
                            chapter: c,
                            verse: v,
                            text: text.clone(),
                        });
                    }
                }
            }
        }
        Ok(out)
    }

    fn label(&self) -> String {
        if !self.meta.abbreviation.is_empty() {
            self.meta.abbreviation.clone()
        } else if !self.meta.name.is_empty() {
            self.meta.name.clone()
        } else {
            "This translation".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Translation {
        let mut t = Translation::new(TranslationMeta {
            abbreviation: "T".into(),
            ..Default::default()
        });
        for c in 1..=2 {
            for v in 1..=5 {
                t.insert(Book::GENESIS, c, v, format!("{c}:{v}"));
            }
        }
        t
    }

    fn texts(t: &Translation, r: &str) -> Vec<String> {
        t.passage(&r.parse().unwrap())
            .unwrap()
            .into_iter()
            .map(|v| v.text)
            .collect()
    }

    #[test]
    fn resolves_ranges_in_order() {
        let t = sample();
        assert_eq!(texts(&t, "Gen 1:4-2:2"), ["1:4", "1:5", "2:1", "2:2"]);
        assert_eq!(texts(&t, "Gen 2"), ["2:1", "2:2", "2:3", "2:4", "2:5"]);
        assert_eq!(texts(&t, "Gen 2:5,1"), ["2:5", "2:1"]);
        assert_eq!(t.verse_count(), 10);
        assert_eq!(t.last_verse(Book::GENESIS, 1), Some(5));
    }

    #[test]
    fn missing_text_is_reported() {
        let t = sample();
        let err = |r: &str| t.passage(&r.parse().unwrap()).unwrap_err().to_string();
        assert_eq!(
            err("Gen 1:9"),
            "Genesis 1 has 5 verses; there is no verse 9"
        );
        assert_eq!(err("Gen 3"), "T does not include Genesis 3");
        assert_eq!(err("Ex 3"), "T does not include Exodus");
    }
}
