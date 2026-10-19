//! Zefania XML Bibles.
//!
//! `<XMLBIBLE><INFORMATION>…</INFORMATION><BIBLEBOOK bnumber><CHAPTER
//! cnumber><VERS vnumber>`. Element names are matched ignoring case. Notes,
//! remarks, captions and cross-references are dropped; books numbered past 66
//! are skipped with a warning.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use lampstand_core::id::{BlobHash, Id};
use lampstand_core::scripture::{BibleText, Licence, Translation};
use quick_xml::events::Event;
use quick_xml::Reader;

use super::osis::{
    attr, collapse, empty_books, licence_from_rights, normalise_language, pos, ref_text, store,
};
use crate::{BibleImport, ImportError};

#[derive(Clone, Copy, PartialEq)]
enum Field {
    Title,
    Identifier,
    Language,
    Rights,
}

fn lower(name: &str) -> String {
    name.to_ascii_lowercase()
}

pub fn import(path: &Path) -> Result<BibleImport, ImportError> {
    let file = File::open(path)?;
    let mut reader = Reader::from_reader(BufReader::new(file));
    let unreadable = |e: &dyn std::fmt::Display| ImportError::Unreadable(e.to_string());

    let mut books = empty_books();
    let mut buf = Vec::new();
    let mut saw_root = false;
    let mut bible_name: Option<String> = None;

    let mut in_info = false;
    let mut field: Option<(Field, String)> = None;
    let (mut title, mut identifier, mut language) = (None, None, None);
    let mut rights: Vec<String> = Vec::new();

    let mut next_book = 0usize;
    let mut book: Option<usize> = None;
    // None while the chapter number is out of range: its verses are skipped.
    let mut chapter = Some(0usize);
    let mut next_chapter = 0usize;
    let mut verse: Option<(usize, usize, usize)> = None;
    let mut text = String::new();
    let mut skip = 0usize;
    let mut skipped_books = Vec::new();
    let mut warnings = Vec::new();

    let finish_verse = |books: &mut Vec<Vec<Vec<String>>>,
                        verse: &mut Option<(usize, usize, usize)>,
                        text: &mut String| {
        if let Some((b, c, v)) = verse.take() {
            store(books, pos(b, c, v), &collapse(text));
        }
        text.clear();
    };

    loop {
        let ev = reader
            .read_event_into(&mut buf)
            .map_err(|e| unreadable(&e))?;
        match ev {
            Event::Eof => break,
            Event::Start(ref e) | Event::Empty(ref e) if !saw_root => {
                if lower(e.local_name().as_ref()) != "xmlbible" {
                    return Err(ImportError::WrongFormat);
                }
                saw_root = true;
                bible_name = attr(e, "biblename").or_else(|| attr(e, "BIBLENAME"));
            }
            Event::Start(ref e) => {
                let n = lower(e.local_name().as_ref());
                if in_info {
                    let f = match n.as_str() {
                        "title" => Some(Field::Title),
                        "identifier" => Some(Field::Identifier),
                        "language" => Some(Field::Language),
                        "rights" => Some(Field::Rights),
                        _ => None,
                    };
                    if let (Some(f), None) = (f, &field) {
                        field = Some((f, String::new()));
                    }
                } else if skip > 0 {
                    skip += 1;
                } else {
                    match n.as_str() {
                        "information" => in_info = true,
                        "biblebook" => {
                            finish_verse(&mut books, &mut verse, &mut text);
                            next_book += 1;
                            let num = attr(e, "bnumber")
                                .and_then(|s| s.trim().parse::<usize>().ok())
                                .unwrap_or(next_book);
                            next_book = num;
                            book = (1..=66).contains(&num).then(|| num - 1);
                            if book.is_none() {
                                skipped_books.push(attr(e, "bname").unwrap_or(num.to_string()));
                            }
                            next_chapter = 0;
                        }
                        "chapter" => {
                            finish_verse(&mut books, &mut verse, &mut text);
                            next_chapter += 1;
                            let num = attr(e, "cnumber")
                                .and_then(|s| s.trim().parse::<usize>().ok())
                                .unwrap_or(next_chapter);
                            next_chapter = num;
                            chapter = if (1..=super::MAX_NUMBER).contains(&num) {
                                Some(num - 1)
                            } else {
                                warnings.push(format!(
                                    "chapter {num} out of range, its verses skipped"
                                ));
                                None
                            };
                        }
                        "vers" => {
                            finish_verse(&mut books, &mut verse, &mut text);
                            let num =
                                attr(e, "vnumber").and_then(|s| s.trim().parse::<usize>().ok());
                            let num = num.filter(|v| (1..=super::MAX_NUMBER).contains(v));
                            if let (Some(b), Some(c), Some(v)) = (book, chapter, num) {
                                verse = Some((b, c, v - 1));
                            }
                        }
                        "note" | "remark" | "caption" | "xref" => skip += 1,
                        "br" => text.push(' '),
                        _ => {}
                    }
                }
            }
            Event::Empty(ref e) => {
                let n = lower(e.local_name().as_ref());
                if skip == 0 && !in_info {
                    match n.as_str() {
                        "vers" => {
                            finish_verse(&mut books, &mut verse, &mut text);
                            let num =
                                attr(e, "vnumber").and_then(|s| s.trim().parse::<usize>().ok());
                            let num = num.filter(|v| (1..=super::MAX_NUMBER).contains(v));
                            if let (Some(b), Some(c), Some(v)) = (book, chapter, num) {
                                store(&mut books, pos(b, c, v - 1), "");
                            }
                        }
                        "br" => text.push(' '),
                        _ => {}
                    }
                }
            }
            Event::End(ref e) => {
                let n = lower(e.local_name().as_ref());
                if in_info {
                    if n == "information" {
                        in_info = false;
                    } else if let Some((f, t)) = field.take() {
                        let t = collapse(&t);
                        match f {
                            Field::Title => {
                                title.get_or_insert(t);
                            }
                            Field::Identifier => {
                                identifier.get_or_insert(t);
                            }
                            Field::Language => {
                                language.get_or_insert(t);
                            }
                            Field::Rights => rights.push(t),
                        }
                    }
                } else if skip > 0 {
                    skip -= 1;
                } else {
                    match n.as_str() {
                        "vers" | "chapter" => finish_verse(&mut books, &mut verse, &mut text),
                        "biblebook" => {
                            finish_verse(&mut books, &mut verse, &mut text);
                            book = None;
                        }
                        _ => {}
                    }
                }
            }
            Event::Text(ref t) => {
                let s = t.xml10_content();
                if in_info {
                    if let Some((_, f)) = field.as_mut() {
                        f.push_str(&s);
                    }
                } else if skip == 0 && verse.is_some() {
                    text.push_str(&s);
                }
            }
            Event::CData(ref t) => {
                let s = t.xml10_content();
                if in_info {
                    if let Some((_, f)) = field.as_mut() {
                        f.push_str(&s);
                    }
                } else if skip == 0 && verse.is_some() {
                    text.push_str(&s);
                }
            }
            Event::GeneralRef(ref r) => {
                if in_info {
                    if let Some((_, f)) = field.as_mut() {
                        f.push_str(&ref_text(r));
                    }
                } else if skip == 0 && verse.is_some() {
                    text.push_str(&ref_text(r));
                }
            }
            _ => {}
        }
        buf.clear();
    }
    finish_verse(&mut books, &mut verse, &mut text);

    if !saw_root {
        return Err(ImportError::WrongFormat);
    }
    if books.iter().all(Vec::is_empty) {
        return Err(ImportError::Unreadable("no verses found".into()));
    }
    if !skipped_books.is_empty() {
        warnings.push(format!(
            "skipped books outside the 66-book canon: {}",
            skipped_books.join(", ")
        ));
    }

    let abbreviation = identifier
        .filter(|s| !s.is_empty())
        .or_else(|| bible_name.clone())
        .unwrap_or_default();
    let name = title
        .filter(|s| !s.is_empty())
        .or(bible_name)
        .unwrap_or_else(|| {
            warnings.push("no title declared; using the abbreviation".into());
            abbreviation.clone()
        });
    let language = language
        .filter(|s| !s.is_empty())
        .map(|l| normalise_language(&l))
        .unwrap_or_else(|| {
            warnings.push("no language declared; using \"und\"".into());
            "und".into()
        });
    let licence = licence_from_rights(&rights).unwrap_or_else(|| {
        warnings.push("no licence declared; treating as licensed".into());
        Licence::Licensed { notice: None }
    });

    Ok(BibleImport {
        translation: Translation {
            id: Id::generate(),
            name,
            abbreviation,
            language,
            licence,
            text: BlobHash::of(&[]),
        },
        text: BibleText { books },
        warnings,
    })
}
