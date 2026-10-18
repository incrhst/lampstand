//! Zefania XML (`<XMLBIBLE>`), common for free translations from the
//! Zefania project and SourceForge. Element names are matched without
//! regard to case, as files in the wild vary.

use super::{number, xml, Builder, Format, Import, ImportError};
use crate::book::Book;
use crate::translation::TranslationMeta;

/// Elements inside a verse whose text is not part of it.
const SKIP: &[&str] = &["note", "xref"];
const BREAKS: &[&str] = &["br"];

pub fn import(source: &str) -> Result<Import, ImportError> {
    let doc = xml::parse(source)?;
    let root = doc.root_element();
    if !xml::is(root, "XMLBIBLE") {
        return Err(ImportError::WrongFormat {
            expected: Format::Zefania,
        });
    }
    let info = xml::child(root, "INFORMATION");
    let get = |name: &str| info.and_then(|i| xml::child_text(i, name));
    let meta = TranslationMeta {
        abbreviation: get("identifier").unwrap_or_default(),
        name: get("title")
            .or_else(|| xml::attr(root, "biblename").map(str::to_string))
            .unwrap_or_default(),
        language: get("language"),
        copyright: get("rights"),
        licence: None,
    };
    let mut builder = Builder::new(Format::Zefania, meta);

    for book_el in root.children().filter(|n| xml::is(*n, "BIBLEBOOK")) {
        let label = xml::attr(book_el, "bname")
            .or_else(|| xml::attr(book_el, "bsname"))
            .or_else(|| xml::attr(book_el, "bnumber"))
            .unwrap_or("?")
            .to_string();
        let book = xml::attr(book_el, "bnumber")
            .and_then(|n| n.trim().parse().ok())
            .and_then(Book::from_number);
        let Some(book) = book else {
            builder.unknown_book(&label);
            continue;
        };
        for chapter_el in book_el.children().filter(|n| xml::is(*n, "CHAPTER")) {
            let Some((chapter, _)) = xml::attr(chapter_el, "cnumber").and_then(number) else {
                builder.skip(label.clone(), "chapter without a number");
                continue;
            };
            for verse_el in chapter_el.children().filter(|n| xml::is(*n, "VERS")) {
                let Some((verse, range)) = xml::attr(verse_el, "vnumber").and_then(number) else {
                    builder.skip(
                        format!("{} {chapter}", book.osis()),
                        "verse without a number",
                    );
                    continue;
                };
                if range {
                    builder.skip(
                        format!("{} {chapter}:{verse}", book.osis()),
                        "joined verses stored as the first",
                    );
                }
                let text = xml::text(verse_el, SKIP, BREAKS);
                builder.verse(book, chapter, verse, &text);
            }
        }
    }
    builder.finish()
}
