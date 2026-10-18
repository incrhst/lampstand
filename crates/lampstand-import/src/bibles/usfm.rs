//! USFM Bibles: a folder of one `.usfm` / `.SFM` file per book.
//!
//! Book numbers come from the `\id` code, names from `\h` (then `\toc2`,
//! `\toc1`, then the canonical English name). Verse text is the text of
//! `\v` up to the next `\v`, `\c` or heading, with footnotes, cross
//! references, attribute lists (`\w word|strong="H1"\w*`) and other
//! character-style markers removed. Every verse of a range (`\v 1-2`) gets the
//! range's text, so lookup by verse number works for each of them.

use std::path::{Path, PathBuf};

use lampstand_core::id::{BlobHash, Id};
use lampstand_core::scripture::{BibleText, Book, Licence, Translation};

use crate::{BibleImport, ImportError};

/// USFM book identifiers in canonical order (book number = index + 1).
const CODES: [&str; 66] = [
    "GEN", "EXO", "LEV", "NUM", "DEU", "JOS", "JDG", "RUT", "1SA", "2SA", "1KI", "2KI", "1CH",
    "2CH", "EZR", "NEH", "EST", "JOB", "PSA", "PRO", "ECC", "SNG", "ISA", "JER", "LAM", "EZK",
    "DAN", "HOS", "JOL", "AMO", "OBA", "JON", "MIC", "NAM", "HAB", "ZEP", "HAG", "ZEC", "MAL",
    "MAT", "MRK", "LUK", "JHN", "ACT", "ROM", "1CO", "2CO", "GAL", "EPH", "PHP", "COL", "1TH",
    "2TH", "1TI", "2TI", "TIT", "PHM", "HEB", "JAS", "1PE", "2PE", "1JN", "2JN", "3JN", "JUD",
    "REV",
];

/// Identifiers for files that hold no Protestant-canon text and are skipped
/// without comment (front and back matter, glossary, apocrypha).
const QUIET_CODES: [&str; 11] = [
    "FRT", "BAK", "GLO", "INT", "TDX", "CNC", "OTH", "XXA", "XXB", "XXC", "XXD",
];

/// Markers whose paragraph text is verse text.
fn is_body_paragraph(name: &str) -> bool {
    let base = name.trim_end_matches(|c: char| c.is_ascii_digit());
    matches!(
        base,
        "p" | "m"
            | "pi"
            | "mi"
            | "nb"
            | "pc"
            | "pr"
            | "ph"
            | "cls"
            | "pm"
            | "pmo"
            | "pmc"
            | "pmr"
            | "q"
            | "qr"
            | "qc"
            | "qm"
            | "qd"
            | "b"
            | "li"
            | "lh"
            | "lf"
            | "lim"
            | "litl"
            | "tr"
            | "tc"
            | "th"
            | "tcr"
            | "thr"
    )
}

/// Inline spans whose whole content is dropped, up to `\name*`.
fn is_dropped_span(name: &str) -> bool {
    matches!(
        name,
        "f" | "fe" | "ef" | "x" | "ex" | "fig" | "ca" | "va" | "vp" | "rq" | "ndx" | "fm"
    )
}

pub fn import(path: &Path) -> Result<BibleImport, ImportError> {
    let files = book_files(path)?;
    if files.is_empty() {
        return Err(ImportError::WrongFormat);
    }

    let mut warnings = Vec::new();
    let mut books: Vec<Vec<Vec<String>>> = vec![Vec::new(); 66];
    let mut seen_book = false;
    let mut id_title: Option<String> = None;
    let mut public_domain = false;

    for file in &files {
        let name = file_name(file);
        let bytes = std::fs::read(file)?;
        let raw = String::from_utf8_lossy(&bytes);
        if raw.contains('\u{FFFD}') {
            warnings.push(format!("{name}: not valid UTF-8, some characters replaced"));
        }
        let raw = raw.trim_start_matches('\u{FEFF}');
        if raw.to_ascii_lowercase().contains("public domain") {
            public_domain = true;
        }

        let Some((code, title)) = read_id(raw) else {
            warnings.push(format!("{name}: skipped, no \\id line"));
            continue;
        };
        let Some(index) = CODES.iter().position(|c| *c == code) else {
            if !QUIET_CODES.contains(&code.as_str()) {
                warnings.push(format!("{name}: skipped, unsupported book code {code}"));
            }
            continue;
        };
        if !books[index].is_empty() {
            warnings.push(format!("{name}: skipped, {code} was already imported"));
            continue;
        }
        let parsed = parse_book(raw, &name, &mut warnings);
        if parsed.chapters.is_empty() {
            warnings.push(format!("{name}: no chapters found"));
            continue;
        }
        seen_book = true;
        if id_title.is_none() && !title.is_empty() {
            id_title = Some(title);
        }
        if let Some(book_name) = parsed.name {
            let canonical = Book::new(index as u8 + 1).map_or("", Book::name);
            if !book_name.eq_ignore_ascii_case(canonical) {
                // Local-language names are kept out of Book for now (Phase 1
                // is English only); say so rather than dropping silently.
                warnings.push(format!(
                    "{name}: book name \"{book_name}\" not stored, using \"{canonical}\""
                ));
            }
        }
        books[index] = parsed.chapters;
    }

    if !seen_book {
        return Err(ImportError::WrongFormat);
    }

    let (translation_name, abbreviation) = translation_names(id_title.as_deref(), path);
    let translation = Translation {
        id: Id::generate(),
        name: translation_name,
        abbreviation,
        language: language_tag(&files),
        licence: if public_domain {
            Licence::PublicDomain
        } else {
            Licence::Licensed { notice: None }
        },
        text: BlobHash::from_bytes([0; 32]),
    };
    Ok(BibleImport {
        translation,
        text: BibleText { books },
        warnings,
    })
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn is_usfm_file(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        let e = e.to_ascii_lowercase();
        e == "usfm" || e == "sfm"
    })
}

fn book_files(path: &Path) -> Result<Vec<PathBuf>, ImportError> {
    if path.is_file() {
        return Ok(if is_usfm_file(path) {
            vec![path.to_path_buf()]
        } else {
            Vec::new()
        });
    }
    let mut files = Vec::new();
    for entry in std::fs::read_dir(path)? {
        let p = entry?.path();
        if p.is_file() && is_usfm_file(&p) {
            files.push(p);
        }
    }
    files.sort();
    Ok(files)
}

/// The book code and the rest of the `\id` line, minus any file name the
/// exporter put there ("JHN 43-JHN-web.sfm World English Bible (WEB)").
fn read_id(raw: &str) -> Option<(String, String)> {
    let line = raw
        .lines()
        .find_map(|l| l.trim_start().strip_prefix("\\id"))?;
    let line = line
        .trim_start_matches(|c: char| c.is_ascii_alphabetic())
        .trim();
    let mut words = line.split_whitespace();
    let code = words.next()?.to_ascii_uppercase();
    let rest: Vec<&str> = words
        .filter(|w| !(w.contains('.') && !w.ends_with('.') && !w.contains('(')))
        .collect();
    Some((code, rest.join(" ")))
}

/// "World English Bible (WEB)" gives ("World English Bible", "WEB").
fn translation_names(id_title: Option<&str>, path: &Path) -> (String, String) {
    let title = id_title.unwrap_or("").trim();
    if let (Some(open), true) = (title.rfind('('), title.ends_with(')')) {
        let abbreviation = title[open + 1..title.len() - 1].trim();
        let name = title[..open].trim();
        if !abbreviation.is_empty() {
            let name = if name.is_empty() { abbreviation } else { name };
            return (name.to_string(), abbreviation.to_string());
        }
    }
    let folder = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = if title.is_empty() {
        folder.clone()
    } else {
        title.to_string()
    };
    (name, folder.to_uppercase())
}

/// eBible.org names files `<lang3><version>`: `engwebp` is English. Other
/// producers give no hint, so default to the three-letter code if there is
/// one and "en" if there is not.
fn language_tag(files: &[PathBuf]) -> String {
    let stem = files
        .first()
        .and_then(|f| f.file_stem())
        .map(|s| s.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let tail = stem.trim_start_matches(|c: char| c.is_ascii_digit() || c == '-');
    let tail = tail.get(3..).unwrap_or("");
    let lang: String = tail.chars().take(3).collect();
    match lang.as_str() {
        "eng" => "en".to_string(),
        "spa" => "es".to_string(),
        "fra" | "fre" => "fr".to_string(),
        "deu" | "ger" => "de".to_string(),
        "por" => "pt".to_string(),
        l if l.len() == 3 && l.chars().all(|c| c.is_ascii_lowercase()) => l.to_string(),
        _ => "en".to_string(),
    }
}

struct ParsedBook {
    name: Option<String>,
    /// `chapters[c][v]` is chapter c+1, verse v+1.
    chapters: Vec<Vec<String>>,
}

/// Where plain text goes at the moment.
#[derive(PartialEq, Clone, Copy)]
enum Sink {
    Ignore,
    Verse,
    Name,
}

struct Builder {
    chapters: Vec<Vec<String>>,
    chapter: usize,
    verse: Option<usize>,
    /// Last verse of a range (`\v 1-2`); equal to `verse` otherwise.
    verse_end: usize,
    text: String,
}

impl Builder {
    fn flush(&mut self) {
        if let (Some(v), Some(chapter)) = (self.verse, self.chapter.checked_sub(1)) {
            let text = self.text.split_whitespace().collect::<Vec<_>>().join(" ");
            let verses = &mut self.chapters[chapter];
            if verses.len() < v {
                verses.resize(v, String::new());
            }
            if verses.len() < self.verse_end {
                verses.resize(self.verse_end, String::new());
            }
            // Every verse of a range carries the range's text.
            for slot in &mut verses[v - 1..self.verse_end.max(v)] {
                if slot.is_empty() {
                    slot.clone_from(&text);
                } else if !text.is_empty() {
                    slot.push(' ');
                    slot.push_str(&text);
                }
            }
        }
        self.text.clear();
    }
}

/// The leading number of a verse or chapter label ("12", "3-4", "5b").
fn leading_number(label: &str) -> Option<usize> {
    let digits: String = label
        .trim()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok().filter(|n| *n > 0)
}

/// The end of a range label: 2 for "1-2" and "1-2a".
fn range_end(label: &str) -> Option<usize> {
    leading_number(label.split_once('-')?.1)
}

fn parse_book(raw: &str, file: &str, warnings: &mut Vec<String>) -> ParsedBook {
    let mut b = Builder {
        chapters: Vec::new(),
        chapter: 0,
        verse: None,
        verse_end: 0,
        text: String::new(),
    };
    let (mut h, mut toc2, mut toc1) = (None::<String>, None::<String>, None::<String>);
    let mut sink = Sink::Ignore;
    let mut name_text = String::new();
    let mut name_target = "";
    let mut rest = raw;

    // Close the heading being collected, if any.
    let finish_name = |target: &str,
                       text: &mut String,
                       h: &mut Option<String>,
                       t2: &mut Option<String>,
                       t1: &mut Option<String>| {
        let value = text.split_whitespace().collect::<Vec<_>>().join(" ");
        text.clear();
        if value.is_empty() {
            return;
        }
        match target {
            "h" => h.get_or_insert(value),
            "toc2" => t2.get_or_insert(value),
            "toc1" => t1.get_or_insert(value),
            _ => return,
        };
    };

    while !rest.is_empty() {
        let Some(pos) = rest.find('\\') else {
            push_text(rest, sink, &mut b, &mut name_text);
            break;
        };
        // Text before the marker, with an attribute list dropped when the
        // next marker closes the span it belongs to.
        let (text, after) = rest.split_at(pos);
        let name_len = after[1..]
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '+' || c == '-'))
            .unwrap_or(after.len() - 1);
        let marker_end = 1 + name_len + usize::from(after[1 + name_len..].starts_with('*'));
        let marker = &after[1..marker_end];
        let is_closer = marker.ends_with('*') || marker.is_empty();
        let text = match (text.find('|'), is_closer) {
            (Some(bar), true) => text[..bar].trim_end(),
            _ => text,
        };
        push_text(text, sink, &mut b, &mut name_text);

        let name = marker.trim_start_matches('+').trim_end_matches('*');
        let mut next = &after[marker_end..];
        if is_closer {
            // `\w*`, `\wj*`, milestone `\*`: nothing to do, text flows on.
            // A space after a closer belongs to the text.
            rest = next;
            continue;
        }
        // The one space after an opening marker belongs to the marker.
        if let Some(stripped) = next.strip_prefix([' ', '\n']) {
            next = stripped;
        }
        if is_dropped_span(name) {
            let closer = format!("\\{name}*");
            match next.find(&closer) {
                Some(end) => next = &next[end + closer.len()..],
                None => {
                    warnings.push(format!(
                        "{file}: unterminated \\{name}, dropped to end of file"
                    ));
                    next = "";
                }
            }
            rest = next;
            continue;
        }
        match name {
            "c" => {
                finish_name(name_target, &mut name_text, &mut h, &mut toc2, &mut toc1);
                b.flush();
                let (label, tail) = split_line(next);
                if let Some(n) = leading_number(label) {
                    if n > b.chapters.len() {
                        b.chapters.resize(n, Vec::new());
                    }
                    b.chapter = n;
                } else {
                    warnings.push(format!("{file}: chapter marker without a number"));
                    b.chapter = 0;
                }
                b.verse = None;
                sink = Sink::Ignore;
                next = tail;
            }
            "v" => {
                finish_name(name_target, &mut name_text, &mut h, &mut toc2, &mut toc1);
                b.flush();
                let trimmed = next.trim_start();
                let end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
                let label = &trimmed[..end];
                next = &trimmed[end..];
                match (leading_number(label), b.chapter) {
                    (Some(n), c) if c > 0 => {
                        b.verse = Some(n);
                        b.verse_end = range_end(label).unwrap_or(n).max(n);
                        sink = Sink::Verse;
                    }
                    _ => {
                        warnings.push(format!("{file}: verse \"{label}\" outside a chapter"));
                        b.verse = None;
                        sink = Sink::Ignore;
                    }
                }
            }
            "h" | "toc1" | "toc2" => {
                finish_name(name_target, &mut name_text, &mut h, &mut toc2, &mut toc1);
                b.flush();
                b.verse = None;
                sink = Sink::Name;
                name_target = match name {
                    "h" => "h",
                    "toc2" => "toc2",
                    _ => "toc1",
                };
            }
            _ => {
                if is_body_paragraph(name) {
                    finish_name(name_target, &mut name_text, &mut h, &mut toc2, &mut toc1);
                    sink = if b.verse.is_some() {
                        Sink::Verse
                    } else {
                        Sink::Ignore
                    };
                    // A paragraph break inside a verse is a space.
                    if sink == Sink::Verse {
                        b.text.push(' ');
                    }
                } else if is_paragraph_level(name) {
                    // Headings, titles, remarks: not verse text.
                    finish_name(name_target, &mut name_text, &mut h, &mut toc2, &mut toc1);
                    b.flush();
                    b.verse = None;
                    sink = Sink::Ignore;
                }
                // Anything else is an inline style (`\wj`, `\add`, `\nd`...)
                // whose text we keep as it is.
            }
        }
        rest = next;
    }
    finish_name(name_target, &mut name_text, &mut h, &mut toc2, &mut toc1);
    b.flush();

    ParsedBook {
        name: h.or(toc2).or(toc1),
        chapters: b.chapters,
    }
}

/// Paragraph-level markers that are not verse text: identification,
/// headings, titles, introductions, remarks, list and table headers.
fn is_paragraph_level(name: &str) -> bool {
    let base = name.trim_end_matches(|c: char| c.is_ascii_digit());
    matches!(
        base,
        "id" | "ide"
            | "sts"
            | "rem"
            | "toc"
            | "toca"
            | "mt"
            | "mte"
            | "ms"
            | "mr"
            | "s"
            | "sr"
            | "r"
            | "d"
            | "sp"
            | "cl"
            | "cp"
            | "cd"
            | "is"
            | "ip"
            | "ipi"
            | "im"
            | "imi"
            | "ipq"
            | "imq"
            | "ipr"
            | "iq"
            | "ib"
            | "iot"
            | "io"
            | "ior"
            | "iex"
            | "imte"
            | "ili"
            | "ie"
            | "usfm"
            | "periph"
            | "restore"
            | "sd"
            | "lik"
            | "liv"
            | "qa"
            | "h"
    )
}

fn push_text(text: &str, sink: Sink, b: &mut Builder, name_text: &mut String) {
    match sink {
        Sink::Verse => b.text.push_str(text),
        Sink::Name => name_text.push_str(text),
        Sink::Ignore => {}
    }
}

/// First line of `s` and the rest (starting at the line break).
fn split_line(s: &str) -> (&str, &str) {
    let end = s.find(['\n', '\\']).unwrap_or(s.len());
    s.split_at(end)
}
