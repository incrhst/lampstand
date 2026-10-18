//! Importing translations the church holds a licence for.
//!
//! PRD: BI-2. Every importer returns the text with its metadata (including
//! any copyright or licence statement the file carries) and a report of
//! anything it left out, so the operator can see what did not come across.

use std::collections::BTreeSet;
use std::fmt;

use crate::book::Book;
use crate::translation::{Translation, TranslationMeta};

pub mod opensong;
pub mod osis;
pub mod usfm;
pub mod zefania;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Osis,
    Usfm,
    Zefania,
    OpenSong,
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Format::Osis => "OSIS",
            Format::Usfm => "USFM",
            Format::Zefania => "Zefania XML",
            Format::OpenSong => "OpenSong",
        })
    }
}

/// Something in the source that did not make it into the translation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    /// Where in the source, e.g. "Tobit" or "Gen 1:1".
    pub location: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImportReport {
    pub skipped: Vec<Skipped>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    pub format: Format,
    pub translation: Translation,
    pub report: ImportReport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportError {
    /// The file is not well-formed XML.
    Xml(String),
    /// The file is XML or text, but not the format expected.
    WrongFormat { expected: Format },
    /// Could not tell which format the file is in.
    UnknownFormat,
    /// The file parsed but held no verses.
    NoVerses { format: Format },
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImportError::Xml(e) => write!(f, "The file is not valid XML: {e}"),
            ImportError::WrongFormat { expected } => write!(f, "This is not a {expected} file"),
            ImportError::UnknownFormat => {
                f.write_str("Could not recognise this file as OSIS, USFM, Zefania XML or OpenSong")
            }
            ImportError::NoVerses { format } => write!(f, "No verses found in this {format} file"),
        }
    }
}

impl std::error::Error for ImportError {}

/// Guess the format from the start of the file.
pub fn detect(source: &str) -> Option<Format> {
    let source = source.trim_start_matches('\u{feff}').trim_start();
    if source.starts_with('\\') {
        return Some(Format::Usfm);
    }
    // The root element is the first tag that is not a declaration, comment
    // or doctype.
    let mut rest = source;
    while let Some(i) = rest.find('<') {
        rest = &rest[i + 1..];
        if rest.starts_with(['?', '!']) {
            continue;
        }
        let name: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == ':' || *c == '_')
            .collect();
        let local = name.rsplit(':').next().unwrap_or("").to_ascii_lowercase();
        return match local.as_str() {
            "osis" => Some(Format::Osis),
            "xmlbible" => Some(Format::Zefania),
            "bible" => Some(Format::OpenSong),
            _ => None,
        };
    }
    None
}

/// Import a file of any supported format, detected from its content. USFM
/// files carry no translation name or licence, so those come back empty.
pub fn import(source: &str) -> Result<Import, ImportError> {
    match detect(source).ok_or(ImportError::UnknownFormat)? {
        Format::Osis => osis::import(source),
        Format::Usfm => usfm::import([source], TranslationMeta::default()),
        Format::Zefania => zefania::import(source),
        Format::OpenSong => opensong::import(source),
    }
}

/// Collects verses for an importer, tidying whitespace and noting
/// duplicates, empty verses and unknown books in the report.
pub(crate) struct Builder {
    format: Format,
    translation: Translation,
    report: ImportReport,
    unknown_books: BTreeSet<String>,
}

impl Builder {
    pub(crate) fn new(format: Format, meta: TranslationMeta) -> Builder {
        Builder {
            format,
            translation: Translation::new(meta),
            report: ImportReport::default(),
            unknown_books: BTreeSet::new(),
        }
    }

    pub(crate) fn skip(&mut self, location: impl Into<String>, reason: impl Into<String>) {
        self.report.skipped.push(Skipped {
            location: location.into(),
            reason: reason.into(),
        });
    }

    /// Report a book the importer does not know, once per book.
    pub(crate) fn unknown_book(&mut self, name: &str) {
        if self.unknown_books.insert(name.to_string()) {
            self.skip(
                name,
                "not one of the 66 books Lampstand knows (apocryphal or unrecognised)",
            );
        }
    }

    pub(crate) fn verse(&mut self, book: Book, chapter: u16, verse: u16, raw: &str) {
        let location = format!("{} {chapter}:{verse}", book.osis());
        if chapter == 0 || verse == 0 {
            self.skip(location, "chapter or verse number 0");
            return;
        }
        let text = tidy(raw);
        if text.is_empty() {
            self.skip(location, "verse has no text");
            return;
        }
        if self
            .translation
            .insert(book, chapter, verse, text)
            .is_some()
        {
            self.skip(location, "verse appears twice; kept the later text");
        }
    }

    pub(crate) fn finish(self) -> Result<Import, ImportError> {
        if self.translation.verse_count() == 0 {
            return Err(ImportError::NoVerses {
                format: self.format,
            });
        }
        Ok(Import {
            format: self.format,
            translation: self.translation,
            report: self.report,
        })
    }
}

/// Collapse runs of whitespace and fix the space a removed note leaves
/// before punctuation.
fn tidy(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for word in raw.split_whitespace() {
        let joins = word.starts_with([',', '.', ';', ':', '!', '?', ')']);
        if !out.is_empty() && !joins {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

/// Parse a verse or chapter number, taking the first of a range such as
/// "1-2". Returns the number and whether it was a range.
pub(crate) fn number(text: &str) -> Option<(u16, bool)> {
    let text = text.trim();
    let digits: String = text.chars().take_while(char::is_ascii_digit).collect();
    let n = digits.parse().ok()?;
    Some((n, digits.len() < text.len()))
}

/// XML helpers shared by the XML formats.
pub(crate) mod xml {
    use roxmltree::{Document, Node, ParsingOptions};

    use super::ImportError;

    pub(crate) fn parse(source: &str) -> Result<Document<'_>, ImportError> {
        let source = source.trim_start_matches('\u{feff}');
        let options = ParsingOptions {
            allow_dtd: true,
            ..Default::default()
        };
        Document::parse_with_options(source, options).map_err(|e| ImportError::Xml(e.to_string()))
    }

    pub(crate) fn is(node: Node<'_, '_>, name: &str) -> bool {
        node.is_element() && node.tag_name().name().eq_ignore_ascii_case(name)
    }

    pub(crate) fn child<'a, 'i>(node: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
        node.children().find(|n| is(*n, name))
    }

    /// Attribute by local name, ignoring case.
    pub(crate) fn attr<'a>(node: Node<'a, '_>, name: &str) -> Option<&'a str> {
        node.attributes()
            .find(|a| a.name().eq_ignore_ascii_case(name))
            .map(|a| a.value())
    }

    /// All text under a node, leaving out the subtrees of elements named in
    /// `skip`; elements in `breaks` become a space.
    pub(crate) fn text(node: Node<'_, '_>, skip: &[&str], breaks: &[&str]) -> String {
        let mut out = String::new();
        collect(node, skip, breaks, &mut out);
        out
    }

    fn collect(node: Node<'_, '_>, skip: &[&str], breaks: &[&str], out: &mut String) {
        for n in node.children() {
            if n.is_text() {
                out.push_str(n.text().unwrap_or(""));
            } else if n.is_element() {
                let name = n.tag_name().name();
                if skip.iter().any(|s| s.eq_ignore_ascii_case(name)) {
                    continue;
                }
                if breaks.iter().any(|s| s.eq_ignore_ascii_case(name)) {
                    out.push(' ');
                }
                collect(n, skip, breaks, out);
            }
        }
    }

    /// Trimmed text of a child element, if present and non-empty.
    pub(crate) fn child_text(node: Node<'_, '_>, name: &str) -> Option<String> {
        let text = super::tidy(&text(child(node, name)?, &[], &[]));
        (!text.is_empty()).then_some(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_formats() {
        assert_eq!(detect("\u{feff}\\id GEN"), Some(Format::Usfm));
        assert_eq!(
            detect("<?xml version=\"1.0\"?>\n<!-- x -->\n<osis xmlns=\"x\">"),
            Some(Format::Osis)
        );
        assert_eq!(
            detect("<XMLBIBLE biblename=\"KJV\">"),
            Some(Format::Zefania)
        );
        assert_eq!(detect("<bible><b n=\"Genesis\">"), Some(Format::OpenSong));
        assert_eq!(detect("<html>"), None);
        assert_eq!(import("hello").unwrap_err(), ImportError::UnknownFormat);
    }

    #[test]
    fn tidies_text() {
        assert_eq!(
            tidy("  In the\n  beginning ,  God  "),
            "In the beginning, God"
        );
    }

    #[test]
    fn numbers() {
        assert_eq!(number("12"), Some((12, false)));
        assert_eq!(number("1-2"), Some((1, true)));
        assert_eq!(number("x"), None);
    }
}
