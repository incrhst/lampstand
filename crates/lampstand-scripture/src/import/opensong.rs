//! OpenSong Bible XML: `<bible><b n="Genesis"><c n="1"><v n="1">`. These
//! files carry no metadata, and books are identified by name.

use super::{number, xml, Builder, Format, Import, ImportError};
use crate::book::Book;
use crate::translation::TranslationMeta;

pub fn import(source: &str) -> Result<Import, ImportError> {
    let doc = xml::parse(source)?;
    let root = doc.root_element();
    if !xml::is(root, "bible") {
        return Err(ImportError::WrongFormat {
            expected: Format::OpenSong,
        });
    }
    let mut builder = Builder::new(Format::OpenSong, TranslationMeta::default());
    let books: Vec<_> = root.children().filter(|n| xml::is(*n, "b")).collect();
    // A complete Bible with names Lampstand cannot read (another language)
    // is matched by position.
    let by_position = books.len() == 66;
    for (i, book_el) in books.into_iter().enumerate() {
        let name = xml::attr(book_el, "n").unwrap_or("").trim();
        let book = Book::parse(name).ok().or_else(|| {
            by_position
                .then(|| Book::from_number(i as u32 + 1))
                .flatten()
        });
        let Some(book) = book else {
            builder.unknown_book(name);
            continue;
        };
        for chapter_el in book_el.children().filter(|n| xml::is(*n, "c")) {
            let Some((chapter, _)) = xml::attr(chapter_el, "n").and_then(number) else {
                builder.skip(name, "chapter without a number");
                continue;
            };
            for verse_el in chapter_el.children().filter(|n| xml::is(*n, "v")) {
                let Some((verse, range)) = xml::attr(verse_el, "n").and_then(number) else {
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
                builder.verse(book, chapter, verse, &xml::text(verse_el, &[], &[]));
            }
        }
    }
    builder.finish()
}
