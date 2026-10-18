//! EasyWorship 6 (`Songs.DB`, Paradox) and 7 (`Songs.db` SQLite, RTF lyrics). OpenLP's `easyworship.py` (GPL) is a reference for both.
//!
//! Two on-disk formats share one entry point, told apart by their first
//! bytes (the file names differ only in case, which Windows and macOS do not
//! preserve):
//!
//! * **SQLite** (EasyWorship 6/7): `Songs.db` holds a `song` table (title,
//!   author, copyright, CCLI number in `vendor_id`). The RTF lyrics are in a
//!   `word` or `song_words` table keyed by `song_id`, either in the same
//!   database or in a sibling `SongWords.db`.
//! * **Paradox** (EasyWorship 2009 and earlier): `Songs.DB` plus a `Songs.MB`
//!   memo file next to it that holds the RTF lyrics.
//!
//! * **Schedules**: `.ewsx` (EasyWorship 6/7 service files: a zip holding a
//!   SQLite `main.db`) and `.ews` (older schedules: fixed-size entries with
//!   zlib-compressed RTF). Only the songs in them are imported.
//!
//! The Paradox reading logic (header layout, field descriptors, block chain,
//! memo block lookup) and the section-heading rules are ported from OpenLP's
//! `openlp/plugins/songs/lib/importers/easyworship.py`, copyright OpenLP
//! Developers, licensed GPL-2.0-or-later, which is compatible with this
//! project's GPL-3.0-or-later. The RTF reader is our own.
//!
//! Lyrics are split into sections on blank lines. A first line that is only
//! a heading ("Verse 2", "Chorus", "Pre-Chorus (soft)") names the section;
//! the heading may carry a number and a parenthesised note, and the note is
//! dropped. Sections without a heading are verses. A section that repeats
//! an earlier one verbatim, or a heading with no lines under it that names
//! an earlier section, becomes an entry in the song's arrangement instead of
//! a duplicate section.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use encoding_rs::Encoding;
use lampstand_core::song::{Section, Song};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};

use crate::{ImportError, Skipped, SongImport};

const SQLITE_MAGIC: &[u8] = b"SQLite format 3\0";
const EWS_MAGIC: &[u8] = b"EasyWorship Schedule File Version";

pub fn import(path: &Path) -> Result<SongImport, ImportError> {
    let file = locate(path)?;
    let mut magic = [0u8; 40];
    {
        use std::io::Read;
        let mut f = fs::File::open(&file)?;
        let n = f.read(&mut magic)?;
        if n < magic.len() {
            magic[n..].fill(0);
        }
    }
    if magic.starts_with(SQLITE_MAGIC) {
        import_sqlite(&file)
    } else if magic.starts_with(b"PK\x03\x04") {
        import_ewsx(&file)
    } else if magic.starts_with(EWS_MAGIC) {
        import_ews(&file)
    } else {
        import_paradox(&file)
    }
}

/// `path` is the database itself, or a folder to look in for `Songs.db`.
fn locate(path: &Path) -> Result<PathBuf, ImportError> {
    if path.is_file() {
        return Ok(path.to_owned());
    }
    if path.is_dir() {
        return find_file(path, "songs.db", 3).ok_or(ImportError::WrongFormat);
    }
    Err(ImportError::Io(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!("{} does not exist", path.display()),
    )))
}

/// Case-insensitive search for a file name, a few folders deep.
fn find_file(dir: &Path, lower_name: &str, depth: u32) -> Option<PathBuf> {
    let mut subdirs = Vec::new();
    let mut entries: Vec<_> = fs::read_dir(dir).ok()?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let p = entry.path();
        if p.is_dir() {
            subdirs.push(p);
        } else if entry.file_name().to_string_lossy().to_lowercase() == lower_name {
            return Some(p);
        }
    }
    if depth == 0 {
        return None;
    }
    subdirs
        .into_iter()
        .find_map(|d| find_file(&d, lower_name, depth - 1))
}

// ---------------------------------------------------------------------------
// Songs
// ---------------------------------------------------------------------------

struct RawSong<'a> {
    title: &'a str,
    author: &'a str,
    copyright: &'a str,
    administrator: &'a str,
    ccli: &'a str,
    /// Plain text from the RTF, or `None` when the song has no lyrics.
    text: Option<String>,
}

/// `Err` carries the reason the song is skipped.
fn build_song(raw: &RawSong) -> Result<Song, String> {
    let text = raw
        .text
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "no song text found".to_string())?;
    let title = raw.title.trim();
    let mut song = Song::new(if title.is_empty() { "Untitled" } else { title });
    let author = raw.author.trim();
    if !author.is_empty() {
        song.author = Some(author.to_string());
    }
    let copyright = raw.copyright.trim();
    let admin = raw.administrator.trim();
    song.copyright = match (copyright.is_empty(), admin.is_empty()) {
        (true, true) => None,
        (false, true) => Some(copyright.to_string()),
        (true, false) => Some(format!("Administered by {admin}")),
        (false, false) => Some(format!("{copyright}, Administered by {admin}")),
    };
    song.ccli_number = parse_ccli(raw.ccli);
    let (sections, arrangement) = split_sections(text);
    if sections.is_empty() {
        return Err("no song text found".to_string());
    }
    song.sections = sections;
    song.arrangement = arrangement;
    Ok(song)
}

/// The first run of digits, if it fits.
fn parse_ccli(s: &str) -> Option<u32> {
    let digits: String = s
        .trim()
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

// ---------------------------------------------------------------------------
// Sections
// ---------------------------------------------------------------------------

/// Heading words, longest first where one is a prefix of another.
const HEADINGS: &[(&str, &str)] = &[
    ("pre-chorus", "Pre-Chorus"),
    ("prechorus", "Pre-Chorus"),
    ("pre chorus", "Pre-Chorus"),
    ("verse", "Verse"),
    ("chorus", "Chorus"),
    ("bridge", "Bridge"),
    ("intro", "Intro"),
    ("ending", "Ending"),
    ("end", "Ending"),
    ("tag", "Tag"),
    ("slide", "Slide"),
    ("other", "Other"),
];

struct Heading {
    name: &'static str,
    number: Option<u32>,
}

/// A line that is nothing but a heading: a word, an optional number and an
/// optional parenthesised note. "Endless love" is a lyric, not "End".
fn parse_heading(line: &str) -> Option<Heading> {
    let lower = line.trim().to_lowercase();
    for &(word, name) in HEADINGS {
        let Some(rest) = lower.strip_prefix(word) else {
            continue;
        };
        let rest = rest.trim();
        let rest = match rest.find('(') {
            Some(i) if rest.ends_with(')') => rest[..i].trim(),
            Some(_) => continue,
            None => rest,
        };
        let rest = rest.trim_end_matches(':').trim();
        if rest.is_empty() {
            return Some(Heading { name, number: None });
        }
        if rest.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(n) = rest.parse() {
                return Some(Heading {
                    name,
                    number: Some(n),
                });
            }
        }
    }
    None
}

fn split_sections(text: &str) -> (Vec<Section>, Vec<String>) {
    let mut blocks: Vec<Vec<&str>> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            if !current.is_empty() {
                blocks.push(std::mem::take(&mut current));
            }
        } else {
            current.push(line.trim_end());
        }
    }
    if !current.is_empty() {
        blocks.push(current);
    }

    let mut sections: Vec<Section> = Vec::new();
    let mut order: Vec<String> = Vec::new();
    for block in blocks {
        let heading = parse_heading(block[0]);
        let body = if heading.is_some() {
            &block[1..]
        } else {
            &block[..]
        };
        let lines: Vec<String> = body.iter().map(|l| l.trim().to_string()).collect();
        let (name, number) = match &heading {
            Some(h) => (h.name, h.number),
            None => ("Verse", None),
        };
        let prefix = format!("{name} ");
        let of_name = |s: &&Section| s.label.starts_with(&prefix);

        // Repeats: identical text under this heading, or a bare heading.
        let existing = if lines.is_empty() {
            match number {
                Some(n) => sections.iter().find(|s| s.label == format!("{name} {n}")),
                None => sections.iter().rev().find(of_name),
            }
        } else {
            sections.iter().filter(of_name).find(|s| s.lines == lines)
        };
        if let Some(s) = existing {
            order.push(s.label.clone());
            continue;
        }

        let used: HashSet<&str> = sections.iter().map(|s| s.label.as_str()).collect();
        let wanted = number.map(|n| format!("{name} {n}"));
        let label = match wanted {
            Some(l) if !used.contains(l.as_str()) => l,
            _ => (1..)
                .map(|n| format!("{name} {n}"))
                .find(|l| !used.contains(l.as_str()))
                .expect("unbounded range"),
        };
        order.push(label.clone());
        sections.push(Section {
            label,
            lines: if lines.is_empty() {
                vec![String::new()]
            } else {
                lines
            },
        });
    }

    // Only keep an arrangement when it says something the written order doesn't.
    let written: Vec<&str> = sections.iter().map(|s| s.label.as_str()).collect();
    let arrangement = if order.iter().map(String::as_str).eq(written.iter().copied()) {
        Vec::new()
    } else {
        order
    };
    (sections, arrangement)
}

// ---------------------------------------------------------------------------
// RTF
// ---------------------------------------------------------------------------

/// Groups whose contents are not text.
const IGNORED_DESTINATIONS: &[&str] = &[
    "fonttbl",
    "colortbl",
    "stylesheet",
    "info",
    "pict",
    "header",
    "footer",
    "headerl",
    "headerr",
    "headerf",
    "footerl",
    "footerr",
    "footerf",
    "listtable",
    "listoverridetable",
    "filetbl",
    "revtbl",
    "rsidtbl",
    "generator",
    "themedata",
    "colorschememapping",
    "latentstyles",
    "datastore",
    "xmlnstbl",
    "mmathpr",
    "fldinst",
    "object",
    "private",
];

fn encoding_for_codepage(cp: u32) -> Option<&'static Encoding> {
    let label = match cp {
        1250..=1258 | 874 => format!("windows-{cp}"),
        932 => "shift_jis".to_string(),
        936 => "gbk".to_string(),
        949 => "euc-kr".to_string(),
        950 => "big5".to_string(),
        65001 => "utf-8".to_string(),
        10000 => "macintosh".to_string(),
        _ => return None,
    };
    Encoding::for_label(label.as_bytes())
}

struct RtfOut {
    text: String,
    /// Bytes from `\'hh` escapes (and raw high bytes), decoded in a run so
    /// multi-byte code pages work.
    bytes: Vec<u8>,
    high_surrogate: Option<u16>,
    encoding: &'static Encoding,
}

impl RtfOut {
    fn flush_bytes(&mut self) {
        if !self.bytes.is_empty() {
            let (s, _, _) = self.encoding.decode(&self.bytes);
            self.text.push_str(&s);
            self.bytes.clear();
        }
    }

    fn push_char(&mut self, c: char) {
        self.flush_bytes();
        self.text.push(c);
    }

    fn push_unit(&mut self, unit: u16) {
        self.flush_bytes();
        match (self.high_surrogate.take(), unit) {
            (Some(hi), lo @ 0xDC00..=0xDFFF) => {
                let c = char::decode_utf16([hi, lo]).next();
                self.text.push(c.and_then(Result::ok).unwrap_or('\u{FFFD}'));
            }
            (_, hi @ 0xD800..=0xDBFF) => self.high_surrogate = Some(hi),
            (_, u) => self
                .text
                .push(char::from_u32(u32::from(u)).unwrap_or('\u{FFFD}')),
        }
    }
}

#[derive(Clone, Copy)]
struct RtfState {
    skip: bool,
    /// `\uc`: how many fallback characters follow each `\u`.
    uc: usize,
}

/// Plain text from RTF: control words, nested groups, `\par`, `\uN` and
/// `\'hh` escapes. Good enough for lyrics, not a general RTF renderer.
///
/// `high_chars_are_bytes` says that chars U+0080..=U+00FF in `rtf` are really
/// single bytes of the document's code page (the Paradox memo case, where the
/// bytes were widened one to one); otherwise they are the characters
/// themselves. `default_encoding` is used for `\'hh` until `\ansicpg` says
/// otherwise.
fn rtf_to_text(
    rtf: &str,
    high_chars_are_bytes: bool,
    default_encoding: &'static Encoding,
) -> String {
    let chars: Vec<char> = rtf.chars().collect();
    let mut out = RtfOut {
        text: String::new(),
        bytes: Vec::new(),
        high_surrogate: None,
        encoding: default_encoding,
    };
    let mut stack = vec![RtfState { skip: false, uc: 1 }];
    let mut skip_fallback = 0usize;
    let mut just_opened = false;
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        i += 1;
        let opened = std::mem::take(&mut just_opened);
        match c {
            '{' => {
                let top = *stack.last().expect("stack never empty");
                stack.push(top);
                just_opened = true;
            }
            '}' => {
                if stack.len() > 1 {
                    stack.pop();
                }
            }
            '\r' | '\n' => {}
            '\\' => {
                let Some(&n) = chars.get(i) else { break };
                i += 1;
                let top = stack.last_mut().expect("stack never empty");
                match n {
                    '\\' | '{' | '}' => {
                        if !top.skip && skip_fallback == 0 {
                            out.push_char(n);
                        }
                        skip_fallback = skip_fallback.saturating_sub(1);
                    }
                    '*' => {
                        if opened {
                            top.skip = true;
                        }
                    }
                    '\'' => {
                        let hex: String = chars.iter().skip(i).take(2).collect();
                        if let Ok(b) = u8::from_str_radix(&hex, 16) {
                            i += 2;
                            if !top.skip {
                                if skip_fallback == 0 {
                                    out.bytes.push(b);
                                } else {
                                    skip_fallback -= 1;
                                }
                            }
                        }
                    }
                    '~' => {
                        if !top.skip {
                            out.push_char(' ');
                        }
                    }
                    '_' => {
                        if !top.skip {
                            out.push_char('-');
                        }
                    }
                    '\r' | '\n' => {
                        if !top.skip {
                            out.push_char('\n');
                        }
                    }
                    c if c.is_ascii_alphabetic() => {
                        let mut word = String::from(c);
                        while let Some(&l) = chars.get(i).filter(|l| l.is_ascii_alphabetic()) {
                            word.push(l);
                            i += 1;
                        }
                        let mut num = String::new();
                        if chars.get(i) == Some(&'-') {
                            num.push('-');
                            i += 1;
                        }
                        while let Some(&d) = chars.get(i).filter(|d| d.is_ascii_digit()) {
                            num.push(d);
                            i += 1;
                        }
                        // One space after a control word is part of it.
                        if chars.get(i) == Some(&' ') {
                            i += 1;
                        }
                        let param: Option<i32> = num.parse().ok();
                        if opened && IGNORED_DESTINATIONS.contains(&word.as_str()) {
                            top.skip = true;
                        }
                        if top.skip {
                            continue;
                        }
                        match word.as_str() {
                            "par" | "line" | "sect" | "page" | "row" => out.push_char('\n'),
                            "tab" | "cell" => out.push_char('\t'),
                            "emdash" => out.push_char('\u{2014}'),
                            "endash" => out.push_char('\u{2013}'),
                            "lquote" => out.push_char('\u{2018}'),
                            "rquote" => out.push_char('\u{2019}'),
                            "ldblquote" => out.push_char('\u{201C}'),
                            "rdblquote" => out.push_char('\u{201D}'),
                            "bullet" => out.push_char('\u{2022}'),
                            "uc" => top.uc = param.unwrap_or(1).max(0) as usize,
                            "ansicpg" => {
                                if let Some(e) = param
                                    .and_then(|p| u32::try_from(p).ok())
                                    .and_then(encoding_for_codepage)
                                {
                                    out.flush_bytes();
                                    out.encoding = e;
                                }
                            }
                            "u" => {
                                if let Some(p) = param {
                                    // Negative values are 16-bit two's complement.
                                    out.push_unit(p as u16);
                                    skip_fallback = top.uc;
                                }
                            }
                            _ => {}
                        }
                    }
                    // Other control symbols (`\-`, `\:`, ...) carry no text.
                    _ => {}
                }
            }
            c => {
                let top = stack.last().expect("stack never empty");
                if top.skip {
                    continue;
                }
                if skip_fallback > 0 {
                    skip_fallback -= 1;
                    continue;
                }
                match u8::try_from(u32::from(c)) {
                    Ok(b) if high_chars_are_bytes && b >= 0x80 => out.bytes.push(b),
                    _ => out.push_char(c),
                }
            }
        }
    }
    out.flush_bytes();
    // Trailing whitespace on each line is noise.
    out.text
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Lyrics are RTF, but accept plain text too.
fn lyrics_text(raw: &str, high_chars_are_bytes: bool, encoding: &'static Encoding) -> String {
    if raw.trim_start().starts_with("{\\rtf") || raw.trim_start().starts_with('{') {
        rtf_to_text(raw, high_chars_are_bytes, encoding)
    } else {
        raw.replace("\r\n", "\n")
    }
}

// ---------------------------------------------------------------------------
// SQLite (EasyWorship 6 and 7)
// ---------------------------------------------------------------------------

fn unreadable(e: impl std::fmt::Display) -> ImportError {
    ImportError::Unreadable(e.to_string())
}

fn table_columns(conn: &Connection, table: &str) -> Result<HashSet<String>, ImportError> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info(\"{table}\")"))
        .map_err(unreadable)?;
    let names = stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map_err(unreadable)?
        .filter_map(Result::ok)
        .map(|n| n.to_lowercase())
        .collect();
    Ok(names)
}

fn value_to_string(v: ValueRef) -> Option<String> {
    match v {
        ValueRef::Null => None,
        ValueRef::Integer(i) => Some(i.to_string()),
        ValueRef::Real(f) => Some(f.to_string()),
        ValueRef::Text(t) | ValueRef::Blob(t) => Some(String::from_utf8_lossy(t).into_owned()),
    }
}

/// `song_id` to RTF, from whichever words table `conn` has.
fn read_words(conn: &Connection) -> Result<Option<HashMap<i64, String>>, ImportError> {
    for table in ["song_words", "word"] {
        let cols = table_columns(conn, table)?;
        if !(cols.contains("song_id") && cols.contains("words")) {
            continue;
        }
        let mut stmt = conn
            .prepare(&format!("SELECT song_id, words FROM \"{table}\""))
            .map_err(unreadable)?;
        let mut rows = stmt.query([]).map_err(unreadable)?;
        let mut map = HashMap::new();
        while let Some(row) = rows.next().map_err(unreadable)? {
            let id = match row.get_ref(0).map_err(unreadable)? {
                ValueRef::Integer(i) => i,
                _ => continue,
            };
            if let Some(w) = value_to_string(row.get_ref(1).map_err(unreadable)?) {
                map.entry(id).or_insert(w);
            }
        }
        return Ok(Some(map));
    }
    Ok(None)
}

fn open_read_only(path: &Path) -> Result<Connection, ImportError> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(unreadable)
}

fn import_sqlite(file: &Path) -> Result<SongImport, ImportError> {
    let conn = open_read_only(file)?;
    let cols = table_columns(&conn, "song")?;
    if !cols.contains("title") {
        return Err(ImportError::WrongFormat);
    }

    let words = match read_words(&conn)? {
        Some(w) => w,
        None => {
            let sibling = file
                .parent()
                .and_then(|d| find_file(d, "songwords.db", 0))
                .ok_or_else(|| unreadable("SongWords.db not found next to Songs.db"))?;
            read_words(&open_read_only(&sibling)?)?
                .ok_or_else(|| unreadable("SongWords.db has no words table"))?
        }
    };

    let pick = |names: &[&str]| -> String {
        names
            .iter()
            .find(|n| cols.contains(**n))
            .map_or("NULL".to_string(), |n| format!("\"{n}\""))
    };
    let query = format!(
        "SELECT rowid, {}, {}, {}, {}, {} FROM song ORDER BY rowid",
        pick(&["title"]),
        pick(&["author"]),
        pick(&["copyright"]),
        pick(&["administrator", "admin"]),
        pick(&["vendor_id", "reference_number", "ccli"]),
    );
    let mut stmt = conn.prepare(&query).map_err(unreadable)?;
    let mut rows = stmt.query([]).map_err(unreadable)?;

    let file_name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut result = SongImport::default();
    while let Some(row) = rows.next().map_err(unreadable)? {
        let get = |i: usize| -> Result<String, ImportError> {
            Ok(value_to_string(row.get_ref(i).map_err(unreadable)?).unwrap_or_default())
        };
        let id: i64 = row.get(0).map_err(unreadable)?;
        let title = get(1)?;
        let raw = RawSong {
            title: &title,
            author: &get(2)?,
            copyright: &get(3)?,
            administrator: &get(4)?,
            ccli: &get(5)?,
            text: words
                .get(&id)
                .map(|w| lyrics_text(w, false, encoding_rs::WINDOWS_1252)),
        };
        match build_song(&raw) {
            Ok(song) => result.songs.push(song),
            Err(reason) => result.skipped.push(Skipped {
                source: format!("{file_name}: {}", title.trim()),
                reason,
            }),
        }
    }
    Ok(result)
}

// ---------------------------------------------------------------------------
// Service files (.ewsx, .ews)
// ---------------------------------------------------------------------------

fn import_ewsx(file: &Path) -> Result<SongImport, ImportError> {
    use std::io::Read;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(fs::read(file)?))
        .map_err(|_| ImportError::WrongFormat)?;
    let mut entry = zip
        .by_name("main.db")
        .map_err(|_| ImportError::WrongFormat)?;
    let mut db = Vec::new();
    // EasyWorship writes a checksum the zip reader rejects, but only after the
    // last byte, so what was read before the error is the whole file.
    let _ = entry.read_to_end(&mut db);
    drop(entry);
    if !db.starts_with(SQLITE_MAGIC) {
        return Err(unreadable("main.db is not a SQLite database"));
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let tmp =
        std::env::temp_dir().join(format!("lampstand-ewsx-{}-{nanos}.db", std::process::id()));
    fs::write(&tmp, &db)?;
    let result = read_ewsx_db(&tmp, file);
    let _ = fs::remove_file(&tmp);
    result
}

fn read_ewsx_db(db: &Path, source: &Path) -> Result<SongImport, ImportError> {
    let conn = open_read_only(db)?;
    if table_columns(&conn, "presentation")?.is_empty() {
        return Err(ImportError::WrongFormat);
    }
    let file_name = source
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    // presentation_type 6 is a song; element_type 6 / element_style_type 4 is
    // a song's lyric text, one RTF resource per slide.
    let mut songs = conn
        .prepare(
            "SELECT rowid, title, author, copyright, reference_number \
             FROM presentation WHERE presentation_type = 6 ORDER BY rowid",
        )
        .map_err(unreadable)?;
    let mut slides = conn
        .prepare(
            "SELECT rt.rtf FROM element AS e \
             JOIN slide AS s ON e.slide_id = s.rowid \
             JOIN resource_text AS rt ON rt.resource_id = e.foreground_resource_id \
             WHERE e.element_type = 6 AND e.element_style_type = 4 AND s.presentation_id = ? \
             ORDER BY s.order_index",
        )
        .map_err(unreadable)?;
    let mut rows = songs.query([]).map_err(unreadable)?;
    let mut result = SongImport::default();
    while let Some(row) = rows.next().map_err(unreadable)? {
        let get = |i: usize| -> Result<String, ImportError> {
            Ok(value_to_string(row.get_ref(i).map_err(unreadable)?).unwrap_or_default())
        };
        let id: i64 = row.get(0).map_err(unreadable)?;
        let title = get(1)?;
        let mut parts = Vec::new();
        let mut rs = slides.query([id]).map_err(unreadable)?;
        while let Some(r) = rs.next().map_err(unreadable)? {
            if let Some(rtf) = value_to_string(r.get_ref(0).map_err(unreadable)?) {
                parts.push(lyrics_text(&rtf, false, encoding_rs::WINDOWS_1252));
            }
        }
        let raw = RawSong {
            title: &title,
            author: &get(2)?,
            copyright: &get(3)?,
            administrator: "",
            ccli: &get(4)?,
            text: (!parts.is_empty()).then(|| parts.join("\n\n")),
        };
        push_song(&mut result, &raw, &file_name);
    }
    Ok(result)
}

fn push_song(result: &mut SongImport, raw: &RawSong, file_name: &str) {
    match build_song(raw) {
        Ok(song) => result.songs.push(song),
        Err(reason) => result.skipped.push(Skipped {
            source: format!("{file_name}: {}", raw.title.trim()),
            reason,
        }),
    }
}

fn le_i32(b: &[u8], off: usize) -> Option<i32> {
    Some(i32::from_le_bytes(b.get(off..off + 4)?.try_into().ok()?))
}

/// A NUL-padded windows-1252 string.
fn fixed_string(b: &[u8], off: usize, len: usize) -> String {
    let Some(raw) = b.get(off..off + len) else {
        return String::new();
    };
    let end = raw.iter().position(|&c| c == 0).unwrap_or(raw.len());
    encoding_rs::WINDOWS_1252
        .decode(&raw[..end])
        .0
        .trim()
        .to_string()
}

/// Format notes: <https://github.com/meinders/lithium-ews/blob/master/docs/ews%20file%20format.md>
fn import_ews(file: &Path) -> Result<SongImport, ImportError> {
    use std::io::Read;
    let data = fs::read(file)?;
    // Room for a header and one entry.
    if data.len() < 892 {
        return Err(unreadable("schedule file is too small"));
    }
    let count_at = match &data[35..38] {
        b"  5" => 56,
        b"  3" => 48,
        b"1.6" => 40,
        _ => return Err(unreadable("unknown schedule file version")),
    };
    let count = le_i32(&data, count_at).unwrap_or(0).max(0) as usize;
    let entry_len = le_u16(&data, count_at + 4).unwrap_or(0) as usize;
    if entry_len < 1420 {
        return Err(unreadable("schedule entries are too short"));
    }
    let first = count_at + 6;
    let count = count.min((data.len().saturating_sub(first)) / entry_len);

    let file_name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut result = SongImport::default();
    for n in 0..count {
        let at = first + n * entry_len;
        // Content type 1 is a song; scripture, media and the rest are not.
        if le_i32(&data, at + 820) != Some(1) {
            continue;
        }
        let title = fixed_string(&data, at, 50);
        let author = fixed_string(&data, at + 307, 50);
        let copyright = fixed_string(&data, at + 358, 100);
        let admin = fixed_string(&data, at + 459, 50);
        let ccli = fixed_string(&data, at + 1410, 10);

        // The content: a length, then zlib data, then 10 bytes of trailer.
        let content = (|| {
            let ptr = usize::try_from(le_i32(&data, at + 800)?).ok()?;
            let len = usize::try_from(le_i32(&data, ptr)?).ok()?;
            let zlib = data.get(ptr + 4..(ptr + 4).checked_add(len.checked_sub(10)?)?)?;
            let mut out = Vec::new();
            flate2::read::ZlibDecoder::new(zlib)
                .take(64 << 20)
                .read_to_end(&mut out)
                .ok()?;
            Some(out)
        })();
        let text = content.map(|bytes| match String::from_utf8(bytes) {
            Ok(s) => lyrics_text(&s, false, encoding_rs::WINDOWS_1252),
            Err(e) => {
                let widened: String = e.as_bytes().iter().map(|&b| char::from(b)).collect();
                lyrics_text(&widened, true, encoding_rs::WINDOWS_1252)
            }
        });
        if text.is_none() {
            result.skipped.push(Skipped {
                source: format!("{file_name}: {title}"),
                reason: "song content could not be decompressed".to_string(),
            });
            continue;
        }
        let raw = RawSong {
            title: &title,
            author: &author,
            copyright: &copyright,
            administrator: &admin,
            ccli: &ccli,
            text,
        };
        push_song(&mut result, &raw, &file_name);
    }
    Ok(result)
}

// ---------------------------------------------------------------------------
// Paradox (EasyWorship 2009 and earlier)
// ---------------------------------------------------------------------------

const PARADOX_HEADER_SIZE: usize = 0x800;

struct Field {
    name: Vec<u8>,
    size: usize,
}

struct ParadoxHeader {
    record_size: usize,
    block_size: usize,
    first_block: usize,
    encoding: &'static Encoding,
    fields: Vec<Field>,
}

fn le_u16(b: &[u8], off: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(off..off + 2)?.try_into().ok()?))
}

fn le_u32(b: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(off..off + 4)?.try_into().ok()?))
}

/// The code page to character set table is OpenLP's; entries other than
/// 852 were guessed there, not observed.
fn paradox_encoding(code_page: u16) -> &'static Encoding {
    let label = match code_page {
        852 => "windows-1250",
        737 | 869 => "windows-1253",
        775 => "windows-1257",
        855 | 866 => "windows-1251",
        857 => "windows-1254",
        862 => "windows-1255",
        874 => "windows-874",
        _ => "windows-1252",
    };
    Encoding::for_label(label.as_bytes()).unwrap_or(encoding_rs::WINDOWS_1252)
}

fn parse_paradox_header(db: &[u8]) -> Result<ParadoxHeader, ImportError> {
    if db.len() < PARADOX_HEADER_SIZE {
        return Err(ImportError::WrongFormat);
    }
    let record_size = le_u16(db, 0).unwrap_or(0) as usize;
    let header_size = le_u16(db, 2).unwrap_or(0) as usize;
    let block_size = db[5] as usize;
    let first_block = le_u16(db, 14).unwrap_or(0) as usize;
    let num_fields = le_u16(db, 33).unwrap_or(0) as usize;
    if header_size != PARADOX_HEADER_SIZE || !(1..=4).contains(&block_size) || record_size == 0 {
        return Err(ImportError::WrongFormat);
    }
    let encoding = paradox_encoding(le_u16(db, 106).unwrap_or(0));

    // Field (type, size) pairs, then 4 + 4n + 261 bytes of other tables, then
    // n NUL-terminated names.
    let info = db
        .get(120..120 + num_fields * 2)
        .ok_or_else(|| unreadable("truncated field descriptors"))?;
    let names_at = 120 + num_fields * 2 + 4 + num_fields * 4 + 261;
    let mut names = db
        .get(names_at..header_size)
        .ok_or_else(|| unreadable("truncated field names"))?
        .split(|&b| b == 0);
    let mut fields = Vec::with_capacity(num_fields);
    for pair in info.as_chunks::<2>().0 {
        let name = names
            .next()
            .ok_or_else(|| unreadable("missing field name"))?;
        fields.push(Field {
            name: name.to_vec(),
            size: pair[1] as usize,
        });
    }
    if fields.iter().map(|f| f.size).sum::<usize>() > record_size {
        return Err(unreadable("fields are larger than the record"));
    }
    Ok(ParadoxHeader {
        record_size,
        block_size,
        first_block,
        encoding,
        fields,
    })
}

/// The bytes a memo/blob field points to in the `.MB` file. `field` is the
/// whole field: inline leader bytes, then a 10-byte pointer (u32 block
/// offset whose low byte is the sub-block, u32 length, 2 more bytes).
fn read_memo<'a>(mb: &'a [u8], field: &'a [u8]) -> &'a [u8] {
    let Some(pointer_at) = field.len().checked_sub(10) else {
        return &[];
    };
    let (Some(start), Some(size)) = (le_u32(field, pointer_at), le_u32(field, pointer_at + 4))
    else {
        return &[];
    };
    let size = size as usize;
    if start == 0 {
        // Short values live in the field itself. OpenLP has no such case;
        // this is from the Paradox format notes and is untested on real files.
        return &field[..pointer_at.min(size)];
    }
    let sub_block = (start & 0xff) as usize;
    let block_start = (start & !0xff) as usize;
    let data_start = match mb.get(block_start) {
        // One large block: 9 byte header.
        Some(2) => block_start + 9,
        // Shared block of 16-byte units: an index of 5 byte entries follows a
        // 12 byte header; the entry's first byte is the unit the data starts at.
        Some(3) if sub_block <= 63 => match mb.get(block_start + 12 + 5 * sub_block) {
            Some(&unit) => block_start + usize::from(unit) * 16,
            None => return &[],
        },
        _ => return &[],
    };
    mb.get(data_start..data_start.saturating_add(size))
        .or_else(|| mb.get(data_start..))
        .unwrap_or(&[])
}

fn find_field(fields: &[Field], name: &str) -> Option<usize> {
    fields
        .iter()
        .position(|f| f.name.eq_ignore_ascii_case(name.as_bytes()))
}

fn import_paradox(file: &Path) -> Result<SongImport, ImportError> {
    let db = fs::read(file)?;
    let header = parse_paradox_header(&db)?;
    let (Some(f_title), Some(f_words)) = (
        find_field(&header.fields, "Title"),
        find_field(&header.fields, "Words"),
    ) else {
        // Some other Paradox table.
        return Err(ImportError::WrongFormat);
    };
    let f_author = find_field(&header.fields, "Author");
    let f_copyright = find_field(&header.fields, "Copyright");
    let f_admin = find_field(&header.fields, "Administrator");
    let f_ccli = find_field(&header.fields, "Song Number");

    let parent = file.parent().unwrap_or(Path::new("."));
    let stem = file
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let mb_path = find_file(parent, &format!("{stem}.mb"), 0)
        .ok_or_else(|| unreadable("the .MB memo file must be next to the .DB file"))?;
    let mb = fs::read(mb_path)?;

    let mut offsets = Vec::with_capacity(header.fields.len());
    let mut at = 0;
    for f in &header.fields {
        offsets.push(at);
        at += f.size;
    }

    let file_name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut result = SongImport::default();
    let text = |rec: &[u8], idx: Option<usize>| -> String {
        let Some(i) = idx else { return String::new() };
        let bytes = &rec[offsets[i]..offsets[i] + header.fields[i].size];
        let end = bytes.iter().rposition(|&b| b != 0).map_or(0, |p| p + 1);
        header.encoding.decode(&bytes[..end]).0.into_owned()
    };

    // Walk the block chain. A block holds a 6-byte header (next block, a
    // reserved word, and the offset of its last record) then whole records.
    let block_bytes = 1024 * header.block_size;
    let mut seen = HashSet::new();
    let mut block = header.first_block;
    while block != 0 && seen.insert(block) {
        let pos = PARADOX_HEADER_SIZE + (block - 1) * block_bytes;
        let (Some(next), Some(last)) = (le_u16(&db, pos), le_u16(&db, pos + 4)) else {
            result.skipped.push(Skipped {
                source: file_name.clone(),
                reason: "database ends inside a block".to_string(),
            });
            break;
        };
        // `last` is a signed offset: an empty block holds -record_size.
        let count = (i32::from(last as i16) + header.record_size as i32)
            .div_euclid(header.record_size as i32)
            .max(0) as usize;
        for n in 0..count {
            let start = pos + 6 + n * header.record_size;
            let Some(rec) = db.get(start..start + header.record_size) else {
                result.skipped.push(Skipped {
                    source: file_name.clone(),
                    reason: "database ends inside a record".to_string(),
                });
                break;
            };
            let title = text(rec, Some(f_title));
            let words_field =
                &rec[offsets[f_words]..offsets[f_words] + header.fields[f_words].size];
            let words = read_memo(&mb, words_field);
            let raw = RawSong {
                title: &title,
                author: &text(rec, f_author),
                copyright: &text(rec, f_copyright),
                administrator: &text(rec, f_admin),
                ccli: &text(rec, f_ccli),
                text: (!words.is_empty()).then(|| {
                    let widened: String = words.iter().map(|&b| char::from(b)).collect();
                    lyrics_text(&widened, true, header.encoding)
                }),
            };
            match build_song(&raw) {
                Ok(song) => result.songs.push(song),
                Err(reason) => result.skipped.push(Skipped {
                    source: format!("{file_name}: {}", title.trim()),
                    reason,
                }),
            }
        }
        block = usize::from(next);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rtf(s: &str) -> String {
        rtf_to_text(s, false, encoding_rs::WINDOWS_1252)
    }

    #[test]
    fn rtf_paragraphs_and_groups() {
        let s = r"{\rtf1\ansi\deff0{\fonttbl{\f0 Arial;}}{\colortbl;\red0\green0\blue0;}\pard Verse 1\par Line one\line two\par\par Chorus\par {\b Bold} line\par}";
        assert_eq!(rtf(s), "Verse 1\nLine one\ntwo\n\nChorus\nBold line");
    }

    #[test]
    fn rtf_escapes() {
        assert_eq!(rtf(r"{\rtf1 caf\'e9 \{x\} a\\b}"), "café {x} a\\b");
        // \u with a fallback character to skip, negative values, \uc0.
        assert_eq!(rtf(r"{\rtf1 \u233?t\u-10179?\u-8704?x}"), "ét\u{1F600}x");
        assert_eq!(rtf(r"{\rtf1\uc0 \u233 e}"), "ée");
        assert_eq!(rtf(r"{\rtf1 a\~b\emdash c\tab d}"), "a b\u{2014}c\td");
    }

    #[test]
    fn rtf_skips_starred_and_ignored_destinations() {
        assert_eq!(
            rtf(r"{\rtf1{\*\generator Riched20;}{\info{\title Secret}}Hello\par}"),
            "Hello"
        );
    }

    #[test]
    fn rtf_code_pages() {
        // 0xE9 is a different letter in windows-1251.
        assert_eq!(rtf(r"{\rtf1\ansicpg1251 \'e9}"), "й");
        // Raw bytes widened one to one, as the Paradox reader does.
        assert_eq!(
            rtf_to_text("{\\rtf1 caf\u{e9}}", true, encoding_rs::WINDOWS_1252),
            "café"
        );
        assert_eq!(
            rtf_to_text("{\\rtf1 caf\u{e9}}", true, encoding_rs::WINDOWS_1251),
            "cafй"
        );
        // Real characters stay as they are.
        assert_eq!(
            rtf_to_text("{\\rtf1 café ✝}", false, encoding_rs::WINDOWS_1252),
            "café ✝"
        );
    }

    #[test]
    fn headings() {
        let h = |s| parse_heading(s).map(|h| (h.name, h.number));
        assert_eq!(h("Verse 2"), Some(("Verse", Some(2))));
        assert_eq!(h("CHORUS"), Some(("Chorus", None)));
        assert_eq!(h("Pre-Chorus 1 (soft)"), Some(("Pre-Chorus", Some(1))));
        assert_eq!(h("Tag"), Some(("Tag", None)));
        assert_eq!(h("Ending"), Some(("Ending", None)));
        assert_eq!(h("Endless love"), None);
        assert_eq!(h("Verses of old"), None);
        assert_eq!(h("Bridge (broken"), None);
    }

    #[test]
    fn sections_and_repeats() {
        let text = "Verse 1\nA\nB\n\nChorus\nC\n\nVerse 2\nD\n\nChorus\nC\n\nChorus\n\nplain\nline\n\n\n\nTag\nT";
        let (sections, arrangement) = split_sections(text);
        let labels: Vec<_> = sections.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(
            labels,
            ["Verse 1", "Chorus 1", "Verse 2", "Verse 3", "Tag 1"]
        );
        assert_eq!(sections[1].lines, ["C"]);
        assert_eq!(
            arrangement,
            ["Verse 1", "Chorus 1", "Verse 2", "Chorus 1", "Chorus 1", "Verse 3", "Tag 1"]
        );
    }

    #[test]
    fn no_arrangement_when_nothing_repeats() {
        let (sections, arrangement) = split_sections("Verse 1\nA\n\nChorus\nB");
        assert_eq!(sections.len(), 2);
        assert!(arrangement.is_empty());
    }

    #[test]
    fn heading_only_section_without_earlier_one_is_an_empty_slide() {
        let (sections, _) = split_sections("Intro\n\nVerse 1\nA");
        assert_eq!(sections[0].label, "Intro 1");
        assert_eq!(sections[0].lines, [""]);
    }

    #[test]
    fn ccli_numbers() {
        assert_eq!(parse_ccli("1234567"), Some(1234567));
        assert_eq!(parse_ccli("CCLI# 22025 "), Some(22025));
        assert_eq!(parse_ccli(""), None);
        assert_eq!(parse_ccli("n/a"), None);
    }

    #[test]
    fn memo_pointer_variants() {
        // Type 2 block: 9-byte header then data.
        let mut mb = vec![0u8; 0x1000];
        let data = b"hello";
        mb[0x800 + 9..0x800 + 9 + 5].copy_from_slice(data);
        mb[0x800] = 2;
        let mut field = vec![0u8; 4];
        field.extend_from_slice(&0x800u32.to_le_bytes());
        field.extend_from_slice(&5u32.to_le_bytes());
        field.extend_from_slice(&[0, 0]);
        assert_eq!(read_memo(&mb, &field), b"hello");

        // Type 3 shared block: sub-block 1 starts at unit 4.
        let mut mb = vec![0u8; 0x1000];
        mb[0x400] = 3;
        mb[0x400 + 12 + 5] = 4;
        mb[0x400 + 64..0x400 + 64 + 3].copy_from_slice(b"abc");
        let mut field = Vec::new();
        field.extend_from_slice(&(0x400u32 | 1).to_le_bytes());
        field.extend_from_slice(&3u32.to_le_bytes());
        field.extend_from_slice(&[0, 0]);
        assert_eq!(read_memo(&mb, &field), b"abc");

        // Garbage pointers don't panic.
        assert_eq!(read_memo(&mb, &[0xff; 10]), b"");
        assert_eq!(read_memo(&mb, &[1, 2, 3]), b"");
    }
}
