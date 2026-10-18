//! OSIS XML (as published by CrossWire and eBible.org). Handles verses both
//! as containers (`<verse osisID>text</verse>`) and as milestones
//! (`<verse sID/>text<verse eID/>`). Notes and headings are left out.

use roxmltree::Node;

use super::{xml, Builder, Format, Import, ImportError};
use crate::book::Book;
use crate::translation::TranslationMeta;

/// Elements whose text is not part of the verse.
const SKIP: &[&str] = &["note", "title", "header", "rdg"];

pub fn import(source: &str) -> Result<Import, ImportError> {
    let doc = xml::parse(source)?;
    let root = doc.root_element();
    if !xml::is(root, "osis") {
        return Err(ImportError::WrongFormat {
            expected: Format::Osis,
        });
    }
    let text_el = xml::child(root, "osisText").unwrap_or(root);
    let mut builder = Builder::new(Format::Osis, meta(text_el));
    let mut walker = Walker {
        builder: &mut builder,
        current: None,
    };
    walker.visit(text_el);
    walker.finish_verse();
    builder.finish()
}

fn meta(text_el: Node<'_, '_>) -> TranslationMeta {
    let work_id = xml::attr(text_el, "osisIDWork").unwrap_or("").to_string();
    let language = text_el
        .attributes()
        .find(|a| a.name() == "lang")
        .map(|a| a.value().to_string());
    let header = xml::child(text_el, "header");
    let work = header.and_then(|h| {
        h.children()
            .filter(|n| xml::is(*n, "work"))
            .find(|w| xml::attr(*w, "osisWork") == Some(work_id.as_str()))
            .or_else(|| xml::child(h, "work"))
    });
    let mut meta = TranslationMeta {
        abbreviation: work_id,
        language,
        ..Default::default()
    };
    if let Some(work) = work {
        meta.name = xml::child_text(work, "title").unwrap_or_default();
        meta.language = meta.language.or_else(|| xml::child_text(work, "language"));
        let rights: Vec<Node<'_, '_>> = work.children().filter(|n| xml::is(*n, "rights")).collect();
        let (licences, copyrights): (Vec<_>, Vec<_>) = rights.iter().partition(|r| {
            xml::attr(**r, "type").is_some_and(|t| t.to_ascii_lowercase().contains("licen"))
        });
        let join = |nodes: Vec<&Node<'_, '_>>| {
            let parts: Vec<String> = nodes
                .iter()
                .filter_map(|n| {
                    let t = super::tidy(&xml::text(**n, &[], &[]));
                    (!t.is_empty()).then_some(t)
                })
                .collect();
            (!parts.is_empty()).then(|| parts.join("\n"))
        };
        meta.copyright = join(copyrights);
        meta.licence = join(licences);
    }
    if meta.name.is_empty() {
        meta.name = meta.abbreviation.clone();
    }
    meta
}

struct Walker<'b> {
    builder: &'b mut Builder,
    current: Option<(Book, u16, u16, String)>,
}

impl Walker<'_> {
    fn visit(&mut self, node: Node<'_, '_>) {
        for n in node.children() {
            if n.is_text() {
                if let Some((.., text)) = &mut self.current {
                    text.push_str(n.text().unwrap_or(""));
                }
                continue;
            }
            if !n.is_element() {
                continue;
            }
            let name = n.tag_name().name();
            if SKIP.iter().any(|s| s.eq_ignore_ascii_case(name)) {
                continue;
            }
            if name == "verse" {
                if xml::attr(n, "eID").is_some() {
                    self.finish_verse();
                } else if let Some(sid) = xml::attr(n, "sID") {
                    self.start_verse(xml::attr(n, "osisID").unwrap_or(sid));
                } else if let Some(id) = xml::attr(n, "osisID") {
                    self.start_verse(id);
                    self.visit(n);
                    self.finish_verse();
                }
                continue;
            }
            if name == "lb" || name == "l" || name == "p" {
                if let Some((.., text)) = &mut self.current {
                    text.push(' ');
                }
            }
            self.visit(n);
        }
    }

    fn start_verse(&mut self, osis_id: &str) {
        self.finish_verse();
        let mut ids = osis_id.split_whitespace();
        let first = ids.next().unwrap_or("");
        let joined = ids.count();
        // Strip a work prefix ("KJV:Gen.1.1") and sub-identifier ("!a").
        let id = first.rsplit(':').next().unwrap_or(first);
        let id = id.split('!').next().unwrap_or(id);
        let parts: Vec<&str> = id.split('.').collect();
        let [book, chapter, verse] = parts[..] else {
            self.builder
                .skip(osis_id, "verse id is not Book.Chapter.Verse");
            return;
        };
        let Some(book) = Book::from_osis(book) else {
            self.builder.unknown_book(book);
            return;
        };
        let (Ok(chapter), Ok(verse)) = (chapter.parse(), verse.parse()) else {
            self.builder
                .skip(osis_id, "verse id has a non-numeric chapter or verse");
            return;
        };
        if joined > 0 {
            self.builder.skip(
                osis_id,
                format!("{} joined verses stored as verse {verse}", joined + 1),
            );
        }
        self.current = Some((book, chapter, verse, String::new()));
    }

    fn finish_verse(&mut self) {
        if let Some((book, chapter, verse, text)) = self.current.take() {
            self.builder.verse(book, chapter, verse, &text);
        }
    }
}
