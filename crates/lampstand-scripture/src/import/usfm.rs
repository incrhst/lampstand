//! USFM (Unified Standard Format Markers), the format of eBible.org and
//! Paratext. One book per file is usual; several `\id` books in one string
//! also work. USFM has no translation name or licence, so the caller
//! supplies them.
//!
//! Kept: verse text, including words of Jesus, added words and divine names
//! (`\wj`, `\add`, `\nd` …) with word attributes (`\w word|strong="H7225"\w*`)
//! removed. Left out: footnotes, cross references, headings, Psalm titles
//! (`\d`), introductions and alternate verse numbers.

use std::collections::BTreeSet;

use super::{number, Builder, Format, Import, ImportError};
use crate::book::Book;
use crate::translation::TranslationMeta;

/// Markers that start a paragraph or line; they separate words but add no
/// text.
const PARAGRAPH: &[&str] = &[
    "p", "m", "po", "pr", "cls", "pmo", "pm", "pmc", "pmr", "pi", "mi", "nb", "pc", "ph", "li",
    "lim", "lh", "lf", "q", "qr", "qc", "qa", "qm", "qd", "b", "tr", "th", "thr", "tc", "tcr",
    "pb", "periph", "esb", "esbe",
];

/// Markers whose content, to the end of the line, is not verse text.
const SKIP_LINE: &[&str] = &[
    "ide", "h", "toc", "toca", "rem", "sts", "usfm", "mt", "mte", "ms", "mr", "s", "sr", "r", "d",
    "sp", "sd", "cl", "cd", "cp", "imt", "imte", "is", "ip", "ipi", "im", "imi", "ipq", "imq",
    "ipr", "iq", "ib", "ili", "iot", "io", "iex", "ie", "lit", "restore",
];

/// Markers whose content, up to their closing `\marker*`, is not verse text.
const SKIP_SPAN: &[&str] = &[
    "f", "fe", "ef", "x", "ex", "fig", "va", "vp", "ca", "cat", "rq",
];

/// Character markers whose content is verse text.
const CHARACTER: &[&str] = &[
    "w", "wj", "add", "nd", "qs", "qac", "tl", "bk", "it", "bd", "bdit", "em", "sc", "no", "sup",
    "pn", "png", "addpn", "k", "sig", "sls", "dc", "lik", "liv", "litl", "ord", "wg", "wh", "wa",
    "rb", "pro", "jmp", "ndx", "xt", "fv", "qt", "ior", "iqt",
];

enum Skip {
    No,
    Line,
    /// Until this closing marker, e.g. "f*".
    Until(String),
}

pub fn import<'a>(
    sources: impl IntoIterator<Item = &'a str>,
    meta: TranslationMeta,
) -> Result<Import, ImportError> {
    let mut parser = Parser {
        builder: Builder::new(Format::Usfm, meta),
        book: None,
        skipping_book: false,
        chapter: 0,
        verse: None,
        skip: Skip::No,
        in_attributes: false,
        unknown_markers: BTreeSet::new(),
    };
    let mut any = false;
    for source in sources {
        let source = source.trim_start_matches('\u{feff}');
        any |= source.trim_start().starts_with('\\');
        parser.run(source);
        parser.end_verse();
        parser.book = None;
        parser.skipping_book = false;
    }
    if !any {
        return Err(ImportError::WrongFormat {
            expected: Format::Usfm,
        });
    }
    parser.builder.finish()
}

struct Parser {
    builder: Builder,
    book: Option<Book>,
    /// Inside a book Lampstand does not know; ignore until the next `\id`.
    skipping_book: bool,
    chapter: u16,
    verse: Option<(u16, String)>,
    skip: Skip,
    /// After `|` in a character marker: word attributes, not text.
    in_attributes: bool,
    unknown_markers: BTreeSet<String>,
}

impl Parser {
    fn run(&mut self, source: &str) {
        let mut chars = source.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            if c == '\\' {
                let start = i + 1;
                let mut end = start;
                while let Some(&(j, d)) = chars.peek() {
                    if d.is_ascii_alphanumeric() || (d == '+' && j == start) || d == '-' {
                        end = j + d.len_utf8();
                        chars.next();
                    } else {
                        break;
                    }
                }
                let closing = matches!(chars.peek(), Some(&(_, '*')));
                // Eat the closing '*', or the one space that ends an opening marker.
                let delimiter = closing
                    || matches!(chars.peek(), Some(&(_, d)) if d.is_whitespace() && d != '\n');
                if delimiter {
                    chars.next();
                }
                let rest = chars.peek().map_or(source.len(), |&(j, _)| j);
                let consumed = self.marker(&source[start..end], closing, &source[rest..]);
                for _ in 0..consumed {
                    chars.next();
                }
                continue;
            }
            match self.skip {
                Skip::Line if c == '\n' => self.skip = Skip::No,
                Skip::No => self.text(c),
                _ => {}
            }
        }
    }

    fn text(&mut self, c: char) {
        if c == '|' {
            self.in_attributes = true;
            return;
        }
        if self.in_attributes {
            return;
        }
        if let Some((_, text)) = &mut self.verse {
            text.push(if c.is_whitespace() { ' ' } else { c });
        }
    }

    /// Handle one marker; returns how many characters of `rest` it used as
    /// its argument (the book code, chapter or verse number).
    fn marker(&mut self, raw: &str, closing: bool, rest: &str) -> usize {
        self.in_attributes = false;
        let name = raw.trim_start_matches('+');
        let base = name.trim_end_matches(|c: char| c.is_ascii_digit() || c == '-');

        // Milestones (\ts\*, \qt-s ...\*) mark positions and hold no text.
        if name.is_empty() || name == "ts" || name.ends_with("-s") || name.ends_with("-e") {
            return 0;
        }
        if let Skip::Until(end) = &self.skip {
            if closing && name == end {
                self.skip = Skip::No;
            }
            return 0;
        }
        let structural = matches!(base, "id" | "c" | "v")
            || PARAGRAPH.contains(&base)
            || SKIP_LINE.contains(&base);
        if let Skip::Line = self.skip {
            if !structural {
                return 0;
            }
            self.skip = Skip::No;
        }
        if closing {
            return 0;
        }

        let (arg, used) = argument(rest);
        match base {
            "id" => {
                self.end_verse();
                self.chapter = 0;
                self.book = Book::from_usfm(arg);
                self.skipping_book = self.book.is_none();
                if self.book.is_none() {
                    self.builder.unknown_book(arg);
                }
                self.skip = Skip::Line;
                used
            }
            "c" => {
                self.end_verse();
                match number(arg) {
                    Some((n, _)) => self.chapter = n,
                    None => self.problem(format!("\\c {arg}"), "chapter number is not a number"),
                }
                used
            }
            "v" => {
                self.end_verse();
                if self.skipping_book {
                    return used;
                }
                let Some(book) = self.book else {
                    self.problem(format!("\\v {arg}"), "verse before any \\id book line");
                    return used;
                };
                match number(arg) {
                    _ if self.chapter == 0 => self.problem(
                        format!("{} \\v {arg}", book.osis()),
                        "verse before any \\c chapter",
                    ),
                    Some((n, range)) => {
                        if range {
                            self.problem(
                                format!("{} {}:{arg}", book.osis(), self.chapter),
                                format!("joined verses stored as verse {n}"),
                            );
                        }
                        self.verse = Some((n, String::new()));
                    }
                    None => self.problem(
                        format!("{} \\v {arg}", book.osis()),
                        "verse number is not a number",
                    ),
                }
                used
            }
            _ if PARAGRAPH.contains(&base) => {
                self.text(' ');
                0
            }
            _ if SKIP_LINE.contains(&base) => {
                self.skip = Skip::Line;
                0
            }
            _ if SKIP_SPAN.contains(&base) => {
                self.skip = Skip::Until(name.to_string());
                0
            }
            _ if CHARACTER.contains(&base) => 0,
            _ => {
                if self.unknown_markers.insert(base.to_string()) {
                    self.builder
                        .skip(format!("\\{base}"), "unknown marker; kept its text");
                }
                0
            }
        }
    }

    fn problem(&mut self, location: String, reason: impl Into<String>) {
        if !self.skipping_book {
            self.builder.skip(location, reason);
        }
    }

    fn end_verse(&mut self) {
        if let (Some(book), Some((verse, text))) = (self.book, self.verse.take()) {
            self.builder.verse(book, self.chapter, verse, &text);
        }
    }
}

/// The first whitespace-separated word of `rest` and the number of
/// characters it and its trailing space take.
fn argument(rest: &str) -> (&str, usize) {
    let trimmed = rest.trim_start_matches([' ', '\t']);
    let lead = rest.len() - trimmed.len();
    let word_len = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
    let word = &trimmed[..word_len];
    let after = &trimmed[word_len..];
    let space = after
        .chars()
        .next()
        .filter(|c| *c == ' ' || *c == '\t')
        .map_or(0, |_| 1);
    (word, rest[..lead + word_len].chars().count() + space)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(source: &str) -> Import {
        import([source], TranslationMeta::default()).unwrap()
    }

    #[test]
    fn strips_notes_and_word_attributes() {
        let i = one(
            "\\id GEN test\n\\c 1\n\\s Creation\n\\p\n\\v 1 \\w In|strong=\"H7225\"\\w* the beginning\\f + \\fr 1.1 \\ft note\\f* God \\add created\\add*.\n\\v 2 And the earth\n\\q2 was \\nd Lord\\nd*.",
        );
        let t = &i.translation;
        assert_eq!(
            t.verse(Book::GENESIS, 1, 1),
            Some("In the beginning God created.")
        );
        assert_eq!(
            t.verse(Book::GENESIS, 1, 2),
            Some("And the earth was Lord.")
        );
        assert!(i.report.skipped.is_empty(), "{:?}", i.report);
    }

    #[test]
    fn reports_unknown_books_once() {
        let i = one("\\id TOB Tobit\n\\c 1\n\\v 1 Tobit text\n\\v 2 More\n\\id JHN\n\\c 1\n\\v 1 In the beginning");
        assert_eq!(i.translation.verse_count(), 1);
        assert_eq!(i.report.skipped.len(), 1);
        assert_eq!(i.report.skipped[0].location, "TOB");
        let e = import(["\\id TOB\n\\c 1\n\\v 1 x\n"], TranslationMeta::default()).unwrap_err();
        assert_eq!(
            e,
            ImportError::NoVerses {
                format: Format::Usfm
            }
        );
    }

    #[test]
    fn ignores_milestones() {
        let i =
            one("\\id JHN\n\\c 1\n\\ts\\*\n\\v 1 In the \\qt-s |who=\"x\"\\*beginning\\qt-e\\*");
        assert_eq!(
            i.translation.verse(Book::JOHN, 1, 1),
            Some("In the beginning")
        );
        assert!(i.report.skipped.is_empty(), "{:?}", i.report);
    }

    #[test]
    fn joined_and_duplicate_verses() {
        let i = one("\\id JHN\n\\c 3\n\\v 16-17 Both.\n\\v 18 One.\n\\v 18 Again.\n\\zz odd\\zz*");
        let reasons: Vec<&str> = i.report.skipped.iter().map(|s| s.reason.as_str()).collect();
        assert_eq!(
            reasons,
            [
                "joined verses stored as verse 16",
                "unknown marker; kept its text",
                "verse appears twice; kept the later text"
            ]
        );
        assert_eq!(i.translation.verse(Book::JOHN, 3, 18), Some("Again. odd"));
    }
}
