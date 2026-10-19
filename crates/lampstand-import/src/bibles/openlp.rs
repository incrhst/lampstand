//! OpenLP Bibles (`.sqlite`, `book` and `verse` tables).
//!
//! Schema, from OpenLP's `openlp/plugins/bibles/lib/db.py` (`BibleMeta`,
//! `Book`, `Verse`; <https://gitlab.com/openlp/openlp>):
//!
//! - `metadata(key, value)`: `name` holds the version name.
//! - `book(id, book_reference_id, testament_reference_id, name)`:
//!   `book_reference_id` is OpenLP's canonical book id (1 = Genesis …
//!   66 = Revelation); we fall back to the book `name` when it is outside
//!   1..=66.
//! - `verse(id, book_id, chapter, verse, text)`: `book_id` is `book.id`.

use std::path::Path;

use lampstand_core::id::{BlobHash, Id};
use lampstand_core::scripture::{BibleText, Licence, Translation};
use rusqlite::{Connection, OpenFlags};

use super::opensong::book_from_name;
use crate::{BibleImport, ImportError};

fn unreadable(e: rusqlite::Error) -> ImportError {
    ImportError::Unreadable(e.to_string())
}

pub fn import(path: &Path) -> Result<BibleImport, ImportError> {
    if !path.is_file() {
        return Err(ImportError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no such file",
        )));
    }
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| ImportError::WrongFormat)?;
    // A non-SQLite file only fails on first read.
    let has = |table: &str| -> Result<bool, ImportError> {
        conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
            [table],
            |r| r.get::<_, i64>(0),
        )
        .map(|n| n > 0)
        .map_err(|_| ImportError::WrongFormat)
    };
    if !(has("book")? && has("verse")?) {
        return Err(ImportError::WrongFormat);
    }

    let mut warnings = Vec::new();
    let mut books: Vec<Vec<Vec<String>>> = vec![Vec::new(); 66];

    // book.id -> 0-based canonical index
    let mut index = std::collections::HashMap::new();
    {
        let mut stmt = conn
            .prepare("SELECT id, book_reference_id, name FROM book")
            .map_err(unreadable)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, Option<i64>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            })
            .map_err(unreadable)?;
        for row in rows {
            let (id, reference, name) = row.map_err(unreadable)?;
            let name = name.unwrap_or_default();
            let canon = reference
                .filter(|r| (1..=66).contains(r))
                .map(|r| r as u8)
                .or_else(|| book_from_name(&name));
            match canon {
                Some(n) => {
                    index.insert(id, usize::from(n) - 1);
                }
                None => warnings.push(format!("unknown book \"{name}\" skipped")),
            }
        }
    }

    {
        let mut stmt = conn
            .prepare("SELECT book_id, chapter, verse, text FROM verse")
            .map_err(unreadable)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, Option<String>>(3)?,
                ))
            })
            .map_err(unreadable)?;
        for row in rows {
            let (book_id, chapter, verse, text) = row.map_err(unreadable)?;
            let Some(&b) = index.get(&book_id) else {
                continue;
            };
            if !(1..1000).contains(&chapter) || !(1..1000).contains(&verse) {
                warnings.push(format!("verse {chapter}:{verse} out of range skipped"));
                continue;
            }
            let (c, v) = (chapter as usize, verse as usize);
            let chapters = &mut books[b];
            if chapters.len() < c {
                chapters.resize(c, Vec::new());
            }
            let verses = &mut chapters[c - 1];
            if verses.len() < v {
                verses.resize(v, String::new());
            }
            verses[v - 1] = text
                .unwrap_or_default()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
        }
    }

    if books.iter().all(|b| b.is_empty()) {
        return Err(ImportError::Unreadable("no verses found".into()));
    }

    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = if has("metadata")? {
        conn.query_row("SELECT value FROM metadata WHERE key='name'", [], |r| {
            r.get::<_, Option<String>>(0)
        })
        .ok()
        .flatten()
        .filter(|n| !n.trim().is_empty())
    } else {
        None
    }
    .unwrap_or_else(|| stem.clone());

    Ok(BibleImport {
        translation: Translation {
            id: Id::generate(),
            name,
            abbreviation: stem,
            language: "en".into(),
            licence: Licence::Licensed { notice: None },
            text: BlobHash::from_bytes([0; 32]),
        },
        text: BibleText { books },
        warnings,
    })
}
