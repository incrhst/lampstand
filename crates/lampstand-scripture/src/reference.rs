//! Free-typed references: "Jn 3:16-18", "1 Cor 13", "Ps 23:1-4,6",
//! "Gen 1:1-2:3".
//!
//! PRD: BI-1.

use std::fmt;
use std::str::FromStr;

use crate::book::{Book, BookError};

/// A chapter, or a verse within it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    pub chapter: u16,
    /// `None` means the whole chapter: verse 1 when it starts a range, the
    /// chapter's last verse when it ends one.
    pub verse: Option<u16>,
}

/// An inclusive run of verses, possibly crossing chapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    pub start: Point,
    pub end: Point,
}

impl Range {
    /// Whether the verse falls inside this range.
    pub fn contains(&self, chapter: u16, verse: u16) -> bool {
        let at = (chapter, verse);
        (self.start.chapter, self.start.verse.unwrap_or(0)) <= at
            && at <= (self.end.chapter, self.end.verse.unwrap_or(u16::MAX))
    }
}

/// A parsed reference: one book and one or more ranges, in the order typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    pub book: Book,
    pub ranges: Vec<Range>,
}

/// Why a typed reference could not be read. `Display` gives a message fit
/// to show the operator as they type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReferenceError {
    Empty,
    Book(BookError),
    /// A book with several chapters and no chapter given ("John").
    MissingChapter(Book),
    NoSuchChapter {
        book: Book,
        chapter: u16,
    },
    /// Chapter or verse 0.
    Zero,
    Backwards {
        book: Book,
        text: String,
    },
    Syntax {
        text: String,
        problem: String,
    },
}

impl fmt::Display for ReferenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReferenceError::Empty => f.write_str("Type a reference, such as John 3:16"),
            ReferenceError::Book(e) => e.fmt(f),
            ReferenceError::MissingChapter(book) => {
                write!(f, "Add a chapter: {book} has {} chapters", book.chapters())
            }
            ReferenceError::NoSuchChapter { book, chapter } => {
                let n = book.chapters();
                let s = if n == 1 { "" } else { "s" };
                write!(
                    f,
                    "{book} has {n} chapter{s}; there is no chapter {chapter}"
                )
            }
            ReferenceError::Zero => f.write_str("Chapters and verses start at 1, not 0"),
            ReferenceError::Backwards { book, text } => {
                write!(
                    f,
                    "\u{201c}{book} {text}\u{201d} runs backwards; put the earlier verse first"
                )
            }
            ReferenceError::Syntax { text, problem } => {
                write!(f, "In \u{201c}{text}\u{201d}: {problem}")
            }
        }
    }
}

impl std::error::Error for ReferenceError {}

impl From<BookError> for ReferenceError {
    fn from(e: BookError) -> Self {
        ReferenceError::Book(e)
    }
}

impl FromStr for Reference {
    type Err = ReferenceError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Reference::parse(s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Token {
    Num(u16),
    /// ':' or '.' between chapter and verse.
    Colon,
    Dash,
    Comma,
    Semicolon,
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Num(n) => write!(f, "{n}"),
            Token::Colon => f.write_str(":"),
            Token::Dash => f.write_str("-"),
            Token::Comma => f.write_str(","),
            Token::Semicolon => f.write_str(";"),
        }
    }
}

impl Reference {
    /// Parse a reference as an operator would type it.
    ///
    /// - `John 3` is the whole chapter, `John 3-4` two chapters.
    /// - `John 3:16-18` is a verse range, `Gen 1:1-2:3` crosses chapters.
    /// - After a verse, a comma adds verses in the same chapter
    ///   (`Ps 23:1-4,6`); a semicolon starts a new chapter (`Jn 3:16; 4:1`).
    /// - In one-chapter books a bare number is a verse (`Jude 3`), and the
    ///   book alone is the whole book.
    /// - `.` works as well as `:`, and en or em dashes as well as `-`.
    pub fn parse(input: &str) -> Result<Reference, ReferenceError> {
        let input = input.trim();
        if input.is_empty() {
            return Err(ReferenceError::Empty);
        }
        let (book_text, spec) = split_book(input);
        let book = Book::parse(book_text)?;
        let spec = spec.trim();
        if spec.is_empty() {
            if book.is_single_chapter() {
                let whole = Point {
                    chapter: 1,
                    verse: None,
                };
                return Ok(Reference {
                    book,
                    ranges: vec![Range {
                        start: whole,
                        end: whole,
                    }],
                });
            }
            return Err(ReferenceError::MissingChapter(book));
        }
        let tokens = tokenise(spec)?;
        let ranges = Parser {
            book,
            spec,
            tokens: &tokens,
            pos: 0,
        }
        .ranges()?;
        Ok(Reference { book, ranges })
    }
}

/// Split "1 Cor 13:4" into ("1 Cor ", "13:4"): the book runs up to the first
/// digit that follows a letter, so a leading book number stays with the book.
fn split_book(input: &str) -> (&str, &str) {
    let mut seen_letter = false;
    for (i, c) in input.char_indices() {
        if c.is_alphabetic() {
            seen_letter = true;
        } else if c.is_ascii_digit() && seen_letter {
            return (&input[..i], &input[i..]);
        }
    }
    (input, "")
}

fn tokenise(spec: &str) -> Result<Vec<Token>, ReferenceError> {
    let mut tokens = Vec::new();
    let mut chars = spec.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        let token = match c {
            c if c.is_whitespace() => continue,
            '0'..='9' => {
                let mut end = i + 1;
                while let Some(&(j, d)) = chars.peek() {
                    if !d.is_ascii_digit() {
                        break;
                    }
                    end = j + 1;
                    chars.next();
                }
                let n = spec[i..end]
                    .parse::<u16>()
                    .map_err(|_| ReferenceError::Syntax {
                        text: spec.to_string(),
                        problem: format!("{} is too large to be a chapter or verse", &spec[i..end]),
                    })?;
                Token::Num(n)
            }
            ':' | '.' => Token::Colon,
            '-' | '\u{2010}'..='\u{2015}' => Token::Dash,
            ',' => Token::Comma,
            ';' => Token::Semicolon,
            other => {
                return Err(ReferenceError::Syntax {
                    text: spec.to_string(),
                    problem: format!(
                    "\u{201c}{other}\u{201d} is not part of a reference; use numbers with : - , ;"
                ),
                })
            }
        };
        tokens.push(token);
    }
    Ok(tokens)
}

struct Parser<'a> {
    book: Book,
    spec: &'a str,
    tokens: &'a [Token],
    pos: usize,
}

impl Parser<'_> {
    fn syntax(&self, problem: impl Into<String>) -> ReferenceError {
        ReferenceError::Syntax {
            text: self.spec.to_string(),
            problem: problem.into(),
        }
    }

    fn peek(&self) -> Option<Token> {
        self.tokens.get(self.pos).copied()
    }

    fn eat(&mut self, t: Token) -> bool {
        if self.peek() == Some(t) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn number(&mut self, after: Option<Token>) -> Result<u16, ReferenceError> {
        match self.peek() {
            Some(Token::Num(0)) => Err(ReferenceError::Zero),
            Some(Token::Num(n)) => {
                self.pos += 1;
                Ok(n)
            }
            found => {
                let what = match after {
                    Some(t) => format!("expected a number after \u{201c}{t}\u{201d}"),
                    None => "expected a chapter number".to_string(),
                };
                Err(self.syntax(match found {
                    Some(t) => format!("{what}, found \u{201c}{t}\u{201d}"),
                    None => what,
                }))
            }
        }
    }

    /// A number, or chapter:verse.
    fn point(&mut self, after: Option<Token>) -> Result<(u16, Option<u16>), ReferenceError> {
        let first = self.number(after)?;
        if self.eat(Token::Colon) {
            Ok((first, Some(self.number(Some(Token::Colon))?)))
        } else {
            Ok((first, None))
        }
    }

    fn check_chapter(&self, chapter: u16) -> Result<u16, ReferenceError> {
        if chapter > self.book.chapters() {
            Err(ReferenceError::NoSuchChapter {
                book: self.book,
                chapter,
            })
        } else {
            Ok(chapter)
        }
    }

    fn ranges(mut self) -> Result<Vec<Range>, ReferenceError> {
        let single = self.book.is_single_chapter();
        // The chapter bare numbers belong to once a verse has been given.
        let mut verse_chapter: Option<u16> = single.then_some(1);
        let mut ranges = Vec::new();
        let mut separator = None;
        loop {
            let item_start = self.pos;
            let start = match self.point(separator)? {
                (c, Some(v)) => Point {
                    chapter: self.check_chapter(c)?,
                    verse: Some(v),
                },
                (v, None) => match verse_chapter {
                    Some(c) => Point {
                        chapter: c,
                        verse: Some(v),
                    },
                    None => Point {
                        chapter: self.check_chapter(v)?,
                        verse: None,
                    },
                },
            };
            let end = if self.eat(Token::Dash) {
                match self.point(Some(Token::Dash))? {
                    (c, Some(v)) => Point {
                        chapter: self.check_chapter(c)?,
                        verse: Some(v),
                    },
                    (n, None) if start.verse.is_some() => Point {
                        chapter: start.chapter,
                        verse: Some(n),
                    },
                    (c, None) => Point {
                        chapter: self.check_chapter(c)?,
                        verse: None,
                    },
                }
            } else {
                start
            };
            let backwards = (end.chapter, end.verse.unwrap_or(u16::MAX))
                < (start.chapter, start.verse.unwrap_or(0));
            if backwards {
                let text: String = self.tokens[item_start..self.pos]
                    .iter()
                    .map(|t| t.to_string())
                    .collect();
                return Err(ReferenceError::Backwards {
                    book: self.book,
                    text,
                });
            }
            verse_chapter = end.verse.map(|_| end.chapter);
            ranges.push(Range { start, end });

            match self.peek() {
                None => return Ok(ranges),
                Some(Token::Comma) => {
                    self.pos += 1;
                    separator = Some(Token::Comma);
                }
                Some(Token::Semicolon) => {
                    self.pos += 1;
                    separator = Some(Token::Semicolon);
                    if !single {
                        verse_chapter = None;
                    }
                }
                Some(t) => {
                    return Err(self.syntax(format!(
                        "did not expect \u{201c}{t}\u{201d} here; separate ranges with , or ;"
                    )))
                }
            }
        }
    }
}

impl fmt::Display for Reference {
    /// Canonical form: "John 3:16\u{2013}18", "Psalms 23:1\u{2013}4, 6",
    /// "Genesis 1:1\u{2013}2:3", "1 Corinthians 13", "Jude 3".
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let single = self.book.is_single_chapter();
        write!(f, "{}", self.book)?;
        let mut prev: Option<Range> = None;
        for r in &self.ranges {
            if single && r.start.verse.is_none() && r.end.verse.is_none() {
                // Whole one-chapter book: the name says it all.
                continue;
            }
            let same_chapter_verses = prev.is_some_and(|p| {
                p.end.verse.is_some() && r.start.verse.is_some() && p.end.chapter == r.start.chapter
            });
            f.write_str(match prev {
                None => " ",
                Some(_) if same_chapter_verses => ", ",
                Some(_) => "; ",
            })?;
            let point = |f: &mut fmt::Formatter<'_>, p: Point, show_chapter: bool| match p.verse {
                Some(v) if show_chapter && !single => write!(f, "{}:{v}", p.chapter),
                Some(v) => write!(f, "{v}"),
                None => write!(f, "{}", p.chapter),
            };
            let (start, end) = if r.end.verse.is_some() && r.start.verse.is_none() {
                (
                    Point {
                        verse: Some(1),
                        ..r.start
                    },
                    r.end,
                )
            } else {
                (r.start, r.end)
            };
            point(f, start, !same_chapter_verses)?;
            if end != start {
                f.write_str("\u{2013}")?;
                point(f, end, end.chapter != start.chapter)?;
            }
            prev = Some(*r);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(chapter: u16, verse: u16) -> Point {
        Point {
            chapter,
            verse: Some(verse),
        }
    }

    fn ch(chapter: u16) -> Point {
        Point {
            chapter,
            verse: None,
        }
    }

    fn r(start: Point, end: Point) -> Range {
        Range { start, end }
    }

    fn parse(s: &str) -> Reference {
        Reference::parse(s).unwrap_or_else(|e| panic!("{s}: {e}"))
    }

    fn err(s: &str) -> String {
        Reference::parse(s).unwrap_err().to_string()
    }

    #[test]
    fn verse_range() {
        let r1 = parse("Jn 3:16-18");
        assert_eq!(r1.book, Book::JOHN);
        assert_eq!(r1.ranges, vec![r(p(3, 16), p(3, 18))]);
        assert_eq!(r1.to_string(), "John 3:16\u{2013}18");
    }

    #[test]
    fn whole_chapter_and_chapter_range() {
        let r1 = parse("1 Cor 13");
        assert_eq!(r1.book.name(), "1 Corinthians");
        assert_eq!(r1.ranges, vec![r(ch(13), ch(13))]);
        assert_eq!(r1.to_string(), "1 Corinthians 13");
        assert_eq!(parse("Jn 3-4").ranges, vec![r(ch(3), ch(4))]);
    }

    #[test]
    fn verse_list_after_comma() {
        let r1 = parse("Ps 23:1-4,6");
        assert_eq!(
            r1.ranges,
            vec![r(p(23, 1), p(23, 4)), r(p(23, 6), p(23, 6))]
        );
        assert_eq!(r1.to_string(), "Psalms 23:1\u{2013}4, 6");
    }

    #[test]
    fn cross_chapter_range() {
        let r1 = parse("Gen 1:1-2:3");
        assert_eq!(r1.ranges, vec![r(p(1, 1), p(2, 3))]);
        assert_eq!(r1.to_string(), "Genesis 1:1\u{2013}2:3");
        assert!(r1.ranges[0].contains(1, 31));
        assert!(r1.ranges[0].contains(2, 3));
        assert!(!r1.ranges[0].contains(2, 4));
        assert_eq!(parse("Gen 1-2:3").ranges, vec![r(ch(1), p(2, 3))]);
    }

    #[test]
    fn semicolon_starts_new_chapter() {
        let r1 = parse("John 3:16; 4:1-2, 7");
        assert_eq!(
            r1.ranges,
            vec![
                r(p(3, 16), p(3, 16)),
                r(p(4, 1), p(4, 2)),
                r(p(4, 7), p(4, 7))
            ]
        );
        assert_eq!(r1.to_string(), "John 3:16; 4:1\u{2013}2, 7");
        assert_eq!(
            parse("Ps 1; 23").ranges,
            vec![r(ch(1), ch(1)), r(ch(23), ch(23))]
        );
    }

    #[test]
    fn loose_typing() {
        assert_eq!(parse("jn3.16").ranges, parse("John 3:16").ranges);
        assert_eq!(
            parse("  1cor 13 : 4 \u{2013} 7 ").ranges,
            vec![r(p(13, 4), p(13, 7))]
        );
        assert_eq!(parse("II Kings 2:11").book.name(), "2 Kings");
        assert_eq!(parse("Song of Songs 2:4").book.name(), "Song of Solomon");
    }

    #[test]
    fn single_chapter_books() {
        assert_eq!(parse("Jude 3").ranges, vec![r(p(1, 3), p(1, 3))]);
        assert_eq!(parse("Jude 3-5").to_string(), "Jude 3\u{2013}5");
        assert_eq!(parse("Jude 1:24-25").ranges, vec![r(p(1, 24), p(1, 25))]);
        assert_eq!(parse("Philemon").ranges, vec![r(ch(1), ch(1))]);
        assert_eq!(parse("Philemon").to_string(), "Philemon");
    }

    #[test]
    fn operator_typos_get_clear_messages() {
        assert_eq!(err(""), "Type a reference, such as John 3:16");
        assert_eq!(err("John"), "Add a chapter: John has 21 chapters");
        assert_eq!(
            err("John 22"),
            "John has 21 chapters; there is no chapter 22"
        );
        assert_eq!(err("Jn 3:0"), "Chapters and verses start at 1, not 0");
        assert_eq!(
            err("Jn 3:18-16"),
            "\u{201c}John 3:18-16\u{201d} runs backwards; put the earlier verse first"
        );
        assert_eq!(
            err("Jn 3:"),
            "In \u{201c}3:\u{201d}: expected a number after \u{201c}:\u{201d}"
        );
        assert_eq!(
            err("Jn 3:16-"),
            "In \u{201c}3:16-\u{201d}: expected a number after \u{201c}-\u{201d}"
        );
        assert_eq!(
            err("Jn 3:16a"),
            "In \u{201c}3:16a\u{201d}: \u{201c}a\u{201d} is not part of a reference; use numbers with : - , ;"
        );
        assert_eq!(
            err("Jn 3:16 17"),
            "In \u{201c}3:16 17\u{201d}: did not expect \u{201c}17\u{201d} here; separate ranges with , or ;"
        );
        assert_eq!(
            err("Jhon 3:16"),
            "There is no book called \u{201c}Jhon\u{201d}. Did you mean John?"
        );
        assert!(err("Jo 3").starts_with("\u{201c}Jo\u{201d} could be"));
    }
}
