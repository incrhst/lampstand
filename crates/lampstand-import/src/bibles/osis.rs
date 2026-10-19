//! OSIS XML Bibles (CrossWire, eBible.org).
//!
//! Streams the file with `quick-xml`. Verse boundaries come from either
//! container verses (`<verse osisID="Gen.1.1">…</verse>`) or milestones
//! (`<verse sID="Gen.1.1"/>…<verse eID="Gen.1.1"/>`); the book, chapter and
//! verse numbers are read from the verse's own OSIS ID, so chapter and book
//! markup is not needed. `<note>` and `<title>` text is dropped. Books
//! outside the 66-book canon are skipped with a warning.
//!
//! The small helpers marked `pub(super)` are shared with the Zefania importer.

use std::collections::BTreeSet;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use lampstand_core::id::{BlobHash, Id};
use lampstand_core::scripture::{BibleText, Licence, Translation};
use quick_xml::events::{BytesRef, BytesStart, Event};
use quick_xml::Reader;

use crate::{BibleImport, ImportError};

const OSIS_BOOKS: [&str; 66] = [
    "Gen", "Exod", "Lev", "Num", "Deut", "Josh", "Judg", "Ruth", "1Sam", "2Sam", "1Kgs", "2Kgs",
    "1Chr", "2Chr", "Ezra", "Neh", "Esth", "Job", "Ps", "Prov", "Eccl", "Song", "Isa", "Jer",
    "Lam", "Ezek", "Dan", "Hos", "Joel", "Amos", "Obad", "Jonah", "Mic", "Nah", "Hab", "Zeph",
    "Hag", "Zech", "Mal", "Matt", "Mark", "Luke", "John", "Acts", "Rom", "1Cor", "2Cor", "Gal",
    "Eph", "Phil", "Col", "1Thess", "2Thess", "1Tim", "2Tim", "Titus", "Phlm", "Heb", "Jas",
    "1Pet", "2Pet", "1John", "2John", "3John", "Jude", "Rev",
];

/// Index (0-based) of an OSIS book ID, ignoring case, with a few common
/// variants.
fn book_index(id: &str) -> Option<usize> {
    let alias = match id.to_ascii_lowercase().as_str() {
        "qoh" => "Eccl",
        "cant" | "sos" => "Song",
        "philem" => "Phlm",
        "jam" => "Jas",
        _ => id,
    };
    OSIS_BOOKS
        .iter()
        .position(|b| b.eq_ignore_ascii_case(alias))
}

/// A verse position: zero-based book, chapter and verse.
#[derive(Clone, Copy)]
pub(super) struct Pos(usize, usize, usize);

pub(super) fn pos(book: usize, chapter: usize, verse: usize) -> Pos {
    Pos(book, chapter, verse)
}

/// Parses the first reference of an OSIS ID ("Gen.1.1", "Gen.1.1-Gen.1.3",
/// "Gen.1.1 Gen.1.2") into its position and the last verse it covers in the
/// same chapter. `Err(Some(book))` is a book outside the canon.
fn parse_osis_id(id: &str) -> Result<(Pos, usize), Option<String>> {
    let first = id.split_whitespace().next().ok_or(None)?;
    let (start, end) = first.split_once('-').unwrap_or((first, first));
    let parse = |s: &str| -> Result<Pos, Option<String>> {
        let mut parts = s.split('.');
        let book = parts.next().ok_or(None)?;
        let b = book_index(book).ok_or_else(|| Some(book.to_string()))?;
        let mut num = || -> Result<usize, Option<String>> {
            let digits: String = parts
                .next()
                .unwrap_or("")
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            digits.parse::<usize>().map_err(|_| None)
        };
        let (c, v) = (num()?, num()?);
        let range = 1..=super::MAX_NUMBER;
        if !range.contains(&c) || !range.contains(&v) {
            return Err(None);
        }
        Ok(Pos(b, c - 1, v - 1))
    };
    let s = parse(start)?;
    let last = match parse(end) {
        Ok(e) if e.0 == s.0 && e.1 == s.1 && e.2 >= s.2 => e.2,
        _ => s.2,
    };
    Ok((s, last))
}

pub(super) fn empty_books() -> Vec<Vec<Vec<String>>> {
    vec![Vec::new(); 66]
}

/// Appends `text` to the verse, growing the chapter and book as needed.
/// Positions past [`super::MAX_NUMBER`] are dropped.
pub(super) fn store(books: &mut [Vec<Vec<String>>], at: Pos, text: &str) {
    if at.1 >= super::MAX_NUMBER || at.2 >= super::MAX_NUMBER {
        return;
    }
    let chapters = &mut books[at.0];
    if chapters.len() <= at.1 {
        chapters.resize_with(at.1 + 1, Vec::new);
    }
    let verses = &mut chapters[at.1];
    if verses.len() <= at.2 {
        verses.resize_with(at.2 + 1, String::new);
    }
    let slot = &mut verses[at.2];
    if !slot.is_empty() && !text.is_empty() {
        slot.push(' ');
    }
    slot.push_str(text);
}

pub(super) fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(super) fn attr(e: &BytesStart<'_>, key: &str) -> Option<String> {
    e.attributes().flatten().find_map(|a| {
        if a.key.local_name().as_ref() == key {
            a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .ok()
                .map(|v| v.into_owned())
        } else {
            None
        }
    })
}

/// A language tag from what Bible files put in `<language>`: common ISO
/// 639-2 codes and English names map to the two-letter code, anything else
/// is lower-cased.
pub(super) fn normalise_language(s: &str) -> String {
    let l = s.trim().to_ascii_lowercase();
    let short = match l.as_str() {
        "eng" | "english" => "en",
        "spa" | "spanish" => "es",
        "fra" | "fre" | "french" => "fr",
        "deu" | "ger" | "german" => "de",
        "por" | "portuguese" => "pt",
        "ita" | "italian" => "it",
        "rus" | "russian" => "ru",
        "nld" | "dut" | "dutch" => "nl",
        "zho" | "chi" | "chinese" => "zh",
        "ara" | "arabic" => "ar",
        "heb" | "hebrew" => "he",
        "ell" | "gre" | "greek" => "el",
        "lat" | "latin" => "la",
        "swa" | "swahili" => "sw",
        _ => return l.replace('_', "-"),
    };
    short.to_string()
}

/// The licence a file declares in its rights statements, if any.
pub(super) fn licence_from_rights(rights: &[String]) -> Option<Licence> {
    let rights: Vec<&str> = rights
        .iter()
        .map(|r| r.trim())
        .filter(|r| !r.is_empty())
        .collect();
    if rights.is_empty() {
        return None;
    }
    if rights
        .iter()
        .any(|r| r.to_ascii_lowercase().contains("public domain"))
    {
        return Some(Licence::PublicDomain);
    }
    Some(Licence::Licensed {
        notice: Some(rights.join(" ")),
    })
}

/// The text of a character or predefined entity reference.
pub(super) fn ref_text(r: &BytesRef<'_>) -> String {
    if let Ok(Some(c)) = r.resolve_char_ref() {
        return c.to_string();
    }
    match r.as_ref() {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" => "'",
        _ => "",
    }
    .to_string()
}

#[derive(Default)]
struct WorkMeta {
    osis_work: Option<String>,
    title: Option<String>,
    identifiers: Vec<(Option<String>, String)>,
    languages: Vec<(Option<String>, String)>,
    rights: Vec<String>,
}

#[derive(Clone, Copy)]
enum Field {
    Title,
    Identifier,
    Language,
    Rights,
}

/// Elements whose text is not scripture.
fn is_dropped(name: &str) -> bool {
    matches!(name, "note" | "title")
}

/// Elements that separate words even when the file has no space around them.
fn is_break(name: &str) -> bool {
    matches!(name, "lb" | "l" | "p" | "item" | "lg")
}

pub fn import(path: &Path) -> Result<BibleImport, ImportError> {
    let file = File::open(path)?;
    let mut reader = Reader::from_reader(BufReader::new(file));
    let unreadable = |e: &dyn std::fmt::Display| ImportError::Unreadable(e.to_string());

    let mut books = empty_books();
    let mut buf = Vec::new();
    let mut saw_root = false;
    let mut id_work: Option<String> = None;

    // Header metadata.
    let mut in_header = false;
    let mut works: Vec<WorkMeta> = Vec::new();
    let mut in_work = false;
    let mut field: Option<(Field, Option<String>, String)> = None;

    // Body state.
    let mut cur: Option<Pos> = None;
    let mut last = 0usize;
    let mut text = String::new();
    let mut skip = 0usize;
    let mut outside_canon: BTreeSet<String> = BTreeSet::new();

    // Ends the open verse, storing its text (and empty slots for the rest of
    // a joined verse such as "Gen.1.1-Gen.1.2").
    macro_rules! close_verse {
        () => {
            if let Some(p) = cur.take() {
                store(&mut books, p, &collapse(&text));
                for v in p.2 + 1..=last {
                    store(&mut books, Pos(p.0, p.1, v), "");
                }
            }
            text.clear();
        };
    }
    macro_rules! open_verse {
        ($id:expr) => {
            match parse_osis_id($id) {
                Ok((p, l)) => {
                    cur = Some(p);
                    last = l;
                }
                Err(Some(b)) => {
                    outside_canon.insert(b);
                }
                Err(None) => {}
            }
        };
    }

    loop {
        let ev = reader
            .read_event_into(&mut buf)
            .map_err(|e| unreadable(&e))?;
        match ev {
            Event::Eof => break,
            Event::Start(ref e) | Event::Empty(ref e) if !saw_root => {
                // The `<osis>` wrapper is optional; `<osisText>` is what counts.
                match e.local_name().as_ref() {
                    "osis" => {}
                    "osisText" => {
                        saw_root = true;
                        id_work = attr(e, "osisIDWork");
                    }
                    _ => return Err(ImportError::WrongFormat),
                }
            }

            // Header: metadata of the work.
            Event::Start(ref e) if in_header => match e.local_name().as_ref() {
                "work" => {
                    in_work = true;
                    works.push(WorkMeta {
                        osis_work: attr(e, "osisWork"),
                        ..Default::default()
                    });
                }
                n if in_work && field.is_none() => {
                    let f = match n {
                        "title" => Some(Field::Title),
                        "identifier" => Some(Field::Identifier),
                        "language" => Some(Field::Language),
                        "rights" => Some(Field::Rights),
                        _ => None,
                    };
                    if let Some(f) = f {
                        field = Some((f, attr(e, "type"), String::new()));
                    }
                }
                _ => {}
            },
            Event::End(ref e) if in_header => match e.local_name().as_ref() {
                "header" => in_header = false,
                "work" => in_work = false,
                _ => {
                    if let (Some((f, ty, t)), Some(w)) = (field.take(), works.last_mut()) {
                        let t = collapse(&t);
                        match f {
                            Field::Title => {
                                w.title.get_or_insert(t);
                            }
                            Field::Identifier => w.identifiers.push((ty, t)),
                            Field::Language => w.languages.push((ty, t)),
                            Field::Rights => w.rights.push(t),
                        }
                    }
                }
            },
            Event::Text(ref t) if in_header => {
                if let Some((_, _, s)) = field.as_mut() {
                    s.push_str(&t.xml10_content());
                }
            }
            Event::CData(ref t) if in_header => {
                if let Some((_, _, s)) = field.as_mut() {
                    s.push_str(&t.xml10_content());
                }
            }
            Event::GeneralRef(ref r) if in_header => {
                if let Some((_, _, s)) = field.as_mut() {
                    s.push_str(&ref_text(r));
                }
            }

            // Body: verses.
            Event::Start(ref e) => {
                let name = e.local_name();
                let n = name.as_ref();
                if n == "header" {
                    in_header = true;
                } else if skip > 0 || is_dropped(n) {
                    skip += 1;
                } else if n == "verse" {
                    close_verse!();
                    if let Some(id) = attr(e, "osisID").or_else(|| attr(e, "sID")) {
                        open_verse!(&id);
                    }
                } else if is_break(n) {
                    text.push(' ');
                }
            }
            Event::Empty(ref e) => {
                let name = e.local_name();
                let n = name.as_ref();
                if skip > 0 {
                    // Inside a dropped element.
                } else if n == "verse" {
                    if let Some(id) = attr(e, "sID") {
                        close_verse!();
                        open_verse!(&id);
                    } else if attr(e, "eID").is_some() {
                        close_verse!();
                    }
                } else if is_break(n) {
                    text.push(' ');
                }
            }
            Event::End(ref e) => {
                let name = e.local_name();
                let n = name.as_ref();
                if skip > 0 {
                    skip -= 1;
                } else if n == "verse" {
                    close_verse!();
                } else if is_break(n) {
                    text.push(' ');
                }
            }
            Event::Text(ref t) if skip == 0 && cur.is_some() => {
                text.push_str(&t.xml10_content());
            }
            Event::CData(ref t) if skip == 0 && cur.is_some() => {
                text.push_str(&t.xml10_content());
            }
            Event::GeneralRef(ref r) if skip == 0 && cur.is_some() => {
                text.push_str(&ref_text(r));
            }
            _ => {}
        }
        buf.clear();
    }
    close_verse!();

    if !saw_root {
        return Err(ImportError::WrongFormat);
    }
    if books.iter().all(Vec::is_empty) {
        return Err(ImportError::Unreadable("no verses found".into()));
    }

    let mut warnings = Vec::new();
    let idx = works
        .iter()
        .position(|w| w.osis_work.is_some() && w.osis_work == id_work)
        .or(if works.is_empty() { None } else { Some(0) });
    let work = idx.map(|i| works.swap_remove(i)).unwrap_or_default();

    let abbreviation = work
        .identifiers
        .iter()
        .find(|(t, v)| {
            t.as_deref().is_some_and(|t| t.eq_ignore_ascii_case("osis")) && !v.is_empty()
        })
        .map(|(_, v)| v.rsplit('.').next().unwrap_or(v).to_string())
        .or(id_work)
        .or(work.osis_work)
        .or_else(|| work.identifiers.first().map(|(_, v)| v.clone()))
        .unwrap_or_default();
    let name = work.title.filter(|t| !t.is_empty()).unwrap_or_else(|| {
        warnings.push("no title declared; using the abbreviation".into());
        abbreviation.clone()
    });
    let language = work
        .languages
        .iter()
        .find(|(t, _)| t.as_deref().is_some_and(|t| t.eq_ignore_ascii_case("ietf")))
        .or(work.languages.first())
        .map(|(_, l)| normalise_language(l))
        .filter(|l| !l.is_empty())
        .unwrap_or_else(|| {
            warnings.push("no language declared; using \"und\"".into());
            "und".into()
        });
    let licence = licence_from_rights(&work.rights).unwrap_or_else(|| {
        warnings.push("no licence declared; treating as licensed".into());
        Licence::Licensed { notice: None }
    });
    if !outside_canon.is_empty() {
        warnings.push(format!(
            "skipped books outside the 66-book canon: {}",
            outside_canon.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }

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
