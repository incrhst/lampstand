//! OpenSong Bibles (`<bible><b><c><v>` XML).
//!
//! ```xml
//! <bible><b n="Genesis"><c n="1"><v n="1">In the beginning…</v></c></b></bible>
//! ```
//!
//! Read with streaming `quick-xml`, so a whole Bible never needs a DOM.
//! Books are matched by name ([`book_from_name`]); a book we cannot place is
//! skipped with a warning. A verse number like `1-2` stores the text at the
//! first number, with the rest left empty.

use std::path::Path;

use lampstand_core::id::{BlobHash, Id};
use lampstand_core::scripture::{BibleText, Licence, Translation, BOOK_NAMES};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use crate::{BibleImport, ImportError};

pub fn import(path: &Path) -> Result<BibleImport, ImportError> {
    let mut reader = Reader::from_file(path).map_err(|e| ImportError::Unreadable(e.to_string()))?;
    let mut buf = Vec::new();
    let mut books: Vec<Vec<Vec<String>>> = vec![Vec::new(); 66];
    let mut warnings = Vec::new();

    let mut seen_root = false;
    let mut name: Option<String> = None;
    let mut book: Option<usize> = None; // index into `books`; None while skipping
    let mut in_book = false;
    let mut chapter: Option<usize> = None;
    let mut verse: Option<usize> = None;
    let mut text = String::new();

    loop {
        let ev = reader
            .read_event_into(&mut buf)
            .map_err(|e| ImportError::Unreadable(e.to_string()))?;
        match ev {
            Event::Start(e) | Event::Empty(e) => match e.name().as_ref() {
                "bible" => {
                    seen_root = true;
                    name = attr(&e, "n").or_else(|| attr(&e, "name"));
                }
                _ if !seen_root => return Err(ImportError::WrongFormat),
                "b" => {
                    in_book = true;
                    chapter = None;
                    let n = attr(&e, "n").unwrap_or_default();
                    book = book_from_name(&n).map(|b| usize::from(b) - 1);
                    if book.is_none() {
                        warnings.push(format!("unknown book \"{n}\" skipped"));
                    }
                }
                "c" if in_book => {
                    chapter = None;
                    if let (Some(b), Some(n)) = (book, number(attr(&e, "n"))) {
                        let chapters = &mut books[b];
                        if chapters.len() < n {
                            chapters.resize(n, Vec::new());
                        }
                        chapter = Some(n - 1);
                    }
                }
                "v" => {
                    text.clear();
                    verse = None;
                    if let (Some(b), Some(c), Some(n)) = (book, chapter, number(attr(&e, "n"))) {
                        let verses = &mut books[b][c];
                        if verses.len() < n {
                            verses.resize(n, String::new());
                        }
                        verse = Some(n - 1);
                    }
                }
                _ => {}
            },
            Event::Text(t) if verse.is_some() => {
                text.push_str(&t.xml11_content());
            }
            Event::GeneralRef(r) if verse.is_some() => {
                if let Ok(Some(c)) = r.resolve_char_ref() {
                    text.push(c);
                } else {
                    match &*r {
                        "amp" => text.push('&'),
                        "lt" => text.push('<'),
                        "gt" => text.push('>'),
                        "quot" => text.push('"'),
                        "apos" => text.push('\''),
                        _ => {}
                    }
                }
            }
            Event::CData(t) if verse.is_some() => {
                text.push_str(&t);
            }
            Event::End(e) => match e.name().as_ref() {
                "v" => {
                    if let (Some(b), Some(c), Some(v)) = (book, chapter, verse) {
                        books[b][c][v] = tidy(&text);
                    }
                    verse = None;
                }
                "c" => chapter = None,
                "b" => {
                    in_book = false;
                    book = None;
                    chapter = None;
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    if !seen_root {
        return Err(ImportError::WrongFormat);
    }
    if books.iter().all(|b| b.is_empty()) {
        return Err(ImportError::Unreadable("no verses found".into()));
    }

    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = name
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| stem.clone());
    Ok(BibleImport {
        translation: Translation {
            id: Id::generate(),
            name,
            abbreviation: stem,
            // OpenSong files carry no language or licence.
            language: "en".into(),
            licence: Licence::Licensed { notice: None },
            text: BlobHash::from_bytes([0; 32]),
        },
        text: BibleText { books },
        warnings,
    })
}

fn attr(e: &BytesStart<'_>, key: &str) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == key)
        .and_then(|a| a.normalized_value(quick_xml::XmlVersion::default()).ok())
        .map(|v| v.into_owned())
}

/// "5" or a range "5-6" (first number wins). Zero is rejected.
fn number(s: Option<String>) -> Option<usize> {
    let s = s?;
    let digits: String = s.trim().chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok().filter(|&n| n > 0 && n < 1000)
}

/// Collapse runs of whitespace (OpenSong files are often hand-wrapped).
fn tidy(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Map an English book name to 1..=66: full names, common abbreviations,
/// and numbered forms ("1 Samuel", "I Samuel", "First Samuel", "1st Sam.").
pub(crate) fn book_from_name(name: &str) -> Option<u8> {
    let lower = name.trim().to_lowercase().replace('.', "");
    let mut words = lower.split_whitespace();
    let first = words.next()?;
    let (prefix, rest): (Option<char>, String) = match first {
        "1" | "i" | "1st" | "first" => (Some('1'), words.collect::<Vec<_>>().join("")),
        "2" | "ii" | "2nd" | "second" => (Some('2'), words.collect::<Vec<_>>().join("")),
        "3" | "iii" | "3rd" | "third" => (Some('3'), words.collect::<Vec<_>>().join("")),
        _ => (
            None,
            std::iter::once(first)
                .chain(words)
                .collect::<Vec<_>>()
                .join(""),
        ),
    };
    // "1john" written without a space.
    let (prefix, rest) = match (prefix, rest.chars().next()) {
        (None, Some(c @ '1'..='3')) if rest.len() > 1 => (Some(c), rest[1..].to_string()),
        _ => (prefix, rest),
    };
    let rest: String = rest.chars().filter(|c| c.is_alphanumeric()).collect();
    if rest.is_empty() {
        return None;
    }
    let key = |n: u8| {
        let full: String = BOOK_NAMES[usize::from(n) - 1]
            .to_lowercase()
            .chars()
            .filter(|c| c.is_alphabetic() || c.is_ascii_digit())
            .collect();
        full
    };
    let want = format!("{}{}", prefix.map(String::from).unwrap_or_default(), rest);

    // Exact full name or alias.
    for n in 1..=66u8 {
        if key(n) == want {
            return Some(n);
        }
    }
    if let Some(n) = alias(&want) {
        return Some(n);
    }
    // Unique prefix of a full name ("Deut", "Phil" is aliased since ambiguous).
    if want.len() >= 3 {
        let hits: Vec<u8> = (1..=66u8).filter(|&n| key(n).starts_with(&want)).collect();
        if let [n] = hits[..] {
            return Some(n);
        }
    }
    None
}

fn alias(k: &str) -> Option<u8> {
    Some(match k {
        "gn" | "gen" => 1,
        "ex" | "exo" | "exod" => 2,
        "lv" | "lev" => 3,
        "nm" | "num" | "nu" => 4,
        "dt" | "deut" | "de" => 5,
        "jos" | "josh" => 6,
        "jdg" | "judg" | "jg" => 7,
        "rth" | "ru" => 8,
        "1sa" | "1sm" | "1sam" => 9,
        "2sa" | "2sm" | "2sam" => 10,
        "1ki" | "1kg" | "1kgs" | "1kin" => 11,
        "2ki" | "2kg" | "2kgs" | "2kin" => 12,
        "1ch" | "1chr" | "1chron" => 13,
        "2ch" | "2chr" | "2chron" => 14,
        "ezr" => 15,
        "neh" | "ne" => 16,
        "est" | "esth" => 17,
        "jb" => 18,
        "psalm" | "ps" | "psa" | "pslm" | "psalms" => 19,
        "prov" | "pr" | "prv" => 20,
        "eccl" | "eccles" | "ecc" | "qoheleth" | "ecclesiasticus" => 21,
        "songofsongs" | "song" | "sos" | "sg" | "ss" | "canticles" | "songofsol" | "cant" => 22,
        "isa" | "is" => 23,
        "jer" => 24,
        "lam" | "la" => 25,
        "ezek" | "eze" | "ezk" => 26,
        "dan" | "dn" => 27,
        "hos" | "ho" => 28,
        "jl" => 29,
        "am" => 30,
        "obad" | "ob" | "oba" => 31,
        "jon" | "jnh" => 32,
        "mic" | "mc" => 33,
        "nah" | "na" => 34,
        "hab" | "hb" => 35,
        "zeph" | "zep" | "zp" => 36,
        "hag" | "hg" => 37,
        "zech" | "zec" | "zc" => 38,
        "mal" | "ml" => 39,
        "matt" | "mt" | "mat" => 40,
        "mrk" | "mk" | "mr" => 41,
        "lk" | "luk" => 42,
        "jhn" | "jn" | "joh" => 43,
        "act" | "ac" => 44,
        "rom" | "rm" | "ro" => 45,
        "1co" | "1cor" => 46,
        "2co" | "2cor" => 47,
        "gal" | "ga" => 48,
        "eph" | "ep" => 49,
        "phil" | "php" | "pp" | "philip" => 50,
        "col" | "co" => 51,
        "1th" | "1thess" | "1thes" => 52,
        "2th" | "2thess" | "2thes" => 53,
        "1ti" | "1tim" => 54,
        "2ti" | "2tim" => 55,
        "tit" | "ti" => 56,
        "phm" | "philem" | "phlm" => 57,
        "heb" => 58,
        "jas" | "jm" => 59,
        "1pe" | "1pet" | "1pt" => 60,
        "2pe" | "2pet" | "2pt" => 61,
        "1jn" | "1jo" | "1joh" | "1jhn" => 62,
        "2jn" | "2jo" | "2joh" | "2jhn" => 63,
        "3jn" | "3jo" | "3joh" | "3jhn" => 64,
        "jud" | "jd" | "jde" => 65,
        "rev" | "re" | "rv" | "revelationofjohn" | "revelations" | "apocalypse" => 66,
        _ => return None,
    })
}
