//! ProPresenter 6 (`.pro6` XML, RTF in base64) and 7 (`.pro` protobuf, using the community proto definitions after checking their licence).
//!
//! The protobuf definitions are vendored from greyshirtguy/ProPresenter7-Proto
//! (MIT) in `proto/propresenter/`; see the README there.
//!
//! Both formats reduce to the same shape: named groups of slides whose text is
//! RTF, plus arrangements that list group ids in sung order. A group becomes a
//! [`Section`] (its slides separated by a blank line, which the paginator reads
//! as a forced break) and the selected arrangement becomes `Song::arrangement`.
//! Backgrounds and slide styling are not carried over.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use base64::Engine;
use prost::Message;

use lampstand_core::song::{Section, Song};

use crate::{ImportError, Skipped, SongImport};

mod pb {
    #![allow(clippy::all, clippy::pedantic, dead_code)]
    include!(concat!(env!("OUT_DIR"), "/rv.data.rs"));
}

/// `path` is a `.pro6` or `.pro` file, or a folder searched recursively for them.
pub fn import(path: &Path) -> Result<SongImport, ImportError> {
    let mut out = SongImport::default();
    if path.is_dir() {
        let mut files = Vec::new();
        collect(path, &mut files)?;
        files.sort();
        for file in files {
            import_file(&file, &mut out)?;
        }
    } else if kind_of(path).is_some() {
        import_file(path, &mut out)?;
    } else {
        return Err(ImportError::WrongFormat);
    }
    Ok(out)
}

#[derive(Clone, Copy)]
enum Kind {
    Pro6,
    Pro7,
}

fn kind_of(path: &Path) -> Option<Kind> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "pro6" => Some(Kind::Pro6),
        "pro" => Some(Kind::Pro7),
        _ => None,
    }
}

fn collect(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), ImportError> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect(&path, files)?;
        } else if kind_of(&path).is_some() {
            files.push(path);
        }
    }
    Ok(())
}

fn import_file(path: &Path, out: &mut SongImport) -> Result<(), ImportError> {
    let kind = kind_of(path).ok_or(ImportError::WrongFormat)?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let bytes = std::fs::read(path)?;
    let stem = path
        .file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let parsed = match kind {
        Kind::Pro6 => parse_pro6(&bytes).map(|mut raw| {
            // A Pro6 document has no title of its own: the file name is the title.
            if !stem.trim().is_empty() {
                raw.title = stem.clone();
            }
            raw
        }),
        Kind::Pro7 => parse_pro7(&bytes),
    };
    match parsed {
        Ok(raw) => match raw.into_song(&stem) {
            Some(song) => out.songs.push(song),
            None => out.skipped.push(Skipped {
                source: name,
                reason: "no slide text".into(),
            }),
        },
        Err(reason) => out.skipped.push(Skipped {
            source: name,
            reason,
        }),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Shared intermediate form

#[derive(Default)]
struct RawSong {
    title: String,
    author: String,
    publisher: String,
    copyright_year: String,
    ccli_number: Option<u32>,
    key: String,
    groups: Vec<RawGroup>,
    arrangements: Vec<RawArrangement>,
    selected: String,
}

struct RawGroup {
    id: String,
    name: String,
    /// One entry per slide: the slide's text, lines joined by `\n`.
    slides: Vec<String>,
}

struct RawArrangement {
    id: String,
    group_ids: Vec<String>,
}

fn non_empty(s: &str) -> Option<String> {
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

impl RawSong {
    fn into_song(self, file_stem: &str) -> Option<Song> {
        let title = non_empty(&self.title).unwrap_or_else(|| file_stem.to_string());
        let mut song = Song::new(title);
        song.author = non_empty(&self.author);
        song.ccli_number = self.ccli_number;
        song.key = non_empty(&self.key);
        let copyright = [self.copyright_year.trim(), self.publisher.trim()]
            .iter()
            .filter(|s| !s.is_empty() && **s != "0")
            .copied()
            .collect::<Vec<_>>()
            .join(" ");
        song.copyright = non_empty(&copyright);

        // Group id -> unique section label. Groups without text are dropped.
        let mut labels: HashMap<String, String> = HashMap::new();
        for group in self.groups {
            let slides: Vec<&str> = group
                .slides
                .iter()
                .map(String::as_str)
                .filter(|s| !s.trim().is_empty())
                .collect();
            if slides.is_empty() {
                continue;
            }
            let base = non_empty(&group.name).unwrap_or_else(|| "Group".into());
            let mut label = base.clone();
            let mut n = 2;
            while song.sections.iter().any(|s| s.label == label) {
                label = format!("{base} {n}");
                n += 1;
            }
            let mut lines = Vec::new();
            for (i, slide) in slides.iter().enumerate() {
                if i > 0 {
                    lines.push(String::new());
                }
                lines.extend(slide.lines().map(str::to_string));
            }
            labels.insert(group.id, label.clone());
            song.sections.push(Section { label, lines });
        }
        if song.sections.is_empty() {
            return None;
        }

        // The selected arrangement; no selection means the written order.
        if let Some(arr) = self
            .arrangements
            .iter()
            .find(|a| !self.selected.is_empty() && a.id == self.selected)
        {
            song.arrangement = arr
                .group_ids
                .iter()
                .filter_map(|id| labels.get(id).cloned())
                .collect();
        }
        Some(song)
    }
}

// ---------------------------------------------------------------------------
// Pro6

fn parse_pro6(bytes: &[u8]) -> Result<RawSong, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "not UTF-8 XML".to_string())?;
    let text = text.trim_start_matches('\u{feff}');
    let doc = roxmltree::Document::parse(text).map_err(|e| format!("invalid XML: {e}"))?;
    let root = doc.root_element();
    if root.tag_name().name() != "RVPresentationDocument" {
        return Err("not a ProPresenter 6 document".into());
    }
    let attr = |name: &str| root.attribute(name).unwrap_or("").to_string();
    let mut raw = RawSong {
        title: non_empty(&attr("CCLISongTitle")).unwrap_or_default(),
        author: attr("CCLIAuthor"),
        publisher: attr("CCLIPublisher"),
        copyright_year: attr("CCLICopyrightYear"),
        ccli_number: attr("CCLISongNumber")
            .trim()
            .parse()
            .ok()
            .filter(|n| *n != 0),
        selected: attr("selectedArrangementID"),
        ..RawSong::default()
    };
    for node in root
        .descendants()
        .filter(|n| n.has_tag_name("RVSlideGrouping"))
    {
        let slides = node
            .descendants()
            .filter(|n| n.has_tag_name("RVDisplaySlide"))
            .filter(|n| !matches!(n.attribute("enabled"), Some("0" | "false")))
            .filter_map(|slide| {
                slide
                    .descendants()
                    .filter(|n| n.attribute("rvXMLIvarName") == Some("RTFData"))
                    .filter_map(|n| decode_rtf_b64(n.text().unwrap_or("")))
                    .find(|t| !t.trim().is_empty())
            })
            .collect();
        raw.groups.push(RawGroup {
            id: node.attribute("uuid").unwrap_or("").to_string(),
            name: node.attribute("name").unwrap_or("").to_string(),
            slides,
        });
    }
    for node in root
        .descendants()
        .filter(|n| n.has_tag_name("RVSongArrangement"))
    {
        let group_ids = node
            .descendants()
            .filter(|n| n.is_element() && n.has_tag_name("NSString"))
            .filter_map(|n| n.text())
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect();
        raw.arrangements.push(RawArrangement {
            id: node.attribute("uuid").unwrap_or("").to_string(),
            group_ids,
        });
    }
    Ok(raw)
}

fn decode_rtf_b64(b64: &str) -> Option<String> {
    let clean: String = b64.chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(clean)
        .ok()?;
    Some(rtf_to_text(&bytes))
}

// ---------------------------------------------------------------------------
// Pro7

fn parse_pro7(bytes: &[u8]) -> Result<RawSong, String> {
    let p = pb::Presentation::decode(bytes).map_err(|e| format!("invalid protobuf: {e}"))?;
    let uuid = |u: &Option<pb::Uuid>| u.as_ref().map(|u| u.string.clone()).unwrap_or_default();

    let mut raw = RawSong {
        title: p.name.clone(),
        key: p.music_key.clone(),
        selected: uuid(&p.selected_arrangement),
        ..RawSong::default()
    };
    if let Some(ccli) = &p.ccli {
        raw.author = ccli.author.clone();
        raw.publisher = ccli.publisher.clone();
        if ccli.copyright_year != 0 {
            raw.copyright_year = ccli.copyright_year.to_string();
        }
        raw.ccli_number = Some(ccli.song_number).filter(|n| *n != 0);
        if raw.title.trim().is_empty() {
            raw.title = ccli.song_title.clone();
        }
    }

    let cue_text: HashMap<String, String> = p
        .cues
        .iter()
        .filter_map(|cue| {
            let text = cue
                .actions
                .iter()
                .filter_map(|a| match &a.action_type_data {
                    Some(pb::action::ActionTypeData::Slide(s)) => Some(s),
                    _ => None,
                })
                .filter_map(|s| match &s.slide {
                    Some(pb::action::slide_type::Slide::Presentation(ps)) => ps.base_slide.as_ref(),
                    _ => None,
                })
                .flat_map(|slide| slide.elements.iter())
                .filter_map(|e| e.element.as_ref()?.text.as_ref())
                .map(|t| rtf_to_text(&t.rtf_data))
                .find(|t| !t.trim().is_empty())?;
            Some((uuid(&cue.uuid), text))
        })
        .collect();

    for cg in &p.cue_groups {
        let group = cg.group.as_ref();
        raw.groups.push(RawGroup {
            id: group.map(|g| uuid(&g.uuid)).unwrap_or_default(),
            name: group.map(|g| g.name.clone()).unwrap_or_default(),
            slides: cg
                .cue_identifiers
                .iter()
                .filter_map(|id| cue_text.get(&id.string).cloned())
                .collect(),
        });
    }
    for a in &p.arrangements {
        raw.arrangements.push(RawArrangement {
            id: uuid(&a.uuid),
            group_ids: a
                .group_identifiers
                .iter()
                .map(|g| g.string.clone())
                .collect(),
        });
    }
    Ok(raw)
}

// ---------------------------------------------------------------------------
// RTF

/// Windows-1252 for 0x80..=0x9F; every other byte is its Latin-1 code point.
const CP1252: [char; 32] = [
    '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}',
    '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
];

fn cp1252(b: u8) -> char {
    match b {
        0x80..=0x9f => CP1252[usize::from(b - 0x80)],
        _ => char::from(b),
    }
}

/// Destinations whose content is never visible text.
const SKIP_DESTINATIONS: &[&str] = &[
    "fonttbl",
    "colortbl",
    "expandedcolortbl",
    "stylesheet",
    "info",
    "pict",
    "header",
    "footer",
    "listtable",
    "listoverridetable",
    "rsidtbl",
    "generator",
    "themedata",
    "colorschememapping",
    "datastore",
    "latentstyles",
    "filetbl",
    "revtbl",
    "private",
    "xmlnstbl",
];

/// Plain text of an RTF document: paragraphs and line breaks become `\n`,
/// `\'hh` and `\uN` escapes are decoded, and font, colour and style tables are
/// dropped. Not a general RTF reader; it covers what ProPresenter writes.
fn rtf_to_text(rtf: &[u8]) -> String {
    let mut out = String::new();
    // Per open group: is its content hidden, and how many characters follow a `\u`.
    let mut stack: Vec<(bool, usize)> = Vec::new();
    let mut hidden = false;
    let mut uc = 1usize;
    let mut skip_chars = 0usize;
    let mut i = 0;
    // The first `{` opens the document group, which is not a destination.
    while i < rtf.len() {
        let b = rtf[i];
        if skip_chars > 0 && b != b'\\' && b != b'{' && b != b'}' {
            skip_chars -= 1;
            i += 1;
            continue;
        }
        match b {
            b'{' => {
                stack.push((hidden, uc));
                i += 1;
                // `{\*\word` is an optional destination; skip it whole.
                if rtf[i..].starts_with(b"\\*") {
                    hidden = true;
                }
            }
            b'}' => {
                if let Some((h, u)) = stack.pop() {
                    hidden = h;
                    uc = u;
                }
                i += 1;
            }
            b'\\' => {
                i += 1;
                let Some(&c) = rtf.get(i) else { break };
                if c.is_ascii_alphabetic() {
                    let start = i;
                    while i < rtf.len() && rtf[i].is_ascii_alphabetic() {
                        i += 1;
                    }
                    let word = std::str::from_utf8(&rtf[start..i]).unwrap_or("");
                    let neg = rtf.get(i) == Some(&b'-');
                    if neg {
                        i += 1;
                    }
                    let nstart = i;
                    while i < rtf.len() && rtf[i].is_ascii_digit() {
                        i += 1;
                    }
                    let num: Option<i32> = std::str::from_utf8(&rtf[nstart..i])
                        .ok()
                        .and_then(|s| s.parse().ok())
                        .map(|n: i32| if neg { -n } else { n });
                    if rtf.get(i) == Some(&b' ') {
                        i += 1;
                    }
                    if SKIP_DESTINATIONS.contains(&word) {
                        hidden = true;
                    }
                    match word {
                        "par" | "line" | "sect" | "page" if !hidden => out.push('\n'),
                        "tab" if !hidden => out.push('\t'),
                        "emdash" if !hidden => out.push('—'),
                        "endash" if !hidden => out.push('–'),
                        "lquote" if !hidden => out.push('‘'),
                        "rquote" if !hidden => out.push('’'),
                        "ldblquote" if !hidden => out.push('“'),
                        "rdblquote" if !hidden => out.push('”'),
                        "bullet" if !hidden => out.push('•'),
                        "uc" => uc = num.and_then(|n| usize::try_from(n).ok()).unwrap_or(1),
                        "u" => {
                            if let Some(n) = num {
                                // Negative values are UTF-16 code units above 0x7FFF.
                                let unit = u32::from(n as u16);
                                let mut ch = char::from_u32(unit);
                                let mut paired = false;
                                // A high surrogate pairs with a following `\uN`.
                                if (0xD800..0xDC00).contains(&unit) {
                                    ch = None;
                                    if let Some((low, next)) = peek_low_surrogate(rtf, i, uc) {
                                        let cp = 0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00);
                                        ch = char::from_u32(cp);
                                        i = next;
                                        paired = true;
                                    }
                                }
                                if !hidden {
                                    out.push(ch.unwrap_or('\u{fffd}'));
                                }
                                skip_chars = if paired { 0 } else { uc };
                            }
                        }
                        _ => {}
                    }
                } else {
                    i += 1;
                    match c {
                        b'\'' => {
                            let hex = rtf.get(i..i + 2).and_then(|h| std::str::from_utf8(h).ok());
                            if let Some(v) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                                if !hidden {
                                    out.push(cp1252(v));
                                }
                                i += 2;
                            }
                        }
                        b'\\' | b'{' | b'}' if !hidden => out.push(char::from(c)),
                        b'\n' | b'\r' if !hidden => out.push('\n'),
                        b'~' if !hidden => out.push('\u{a0}'),
                        b'_' if !hidden => out.push('‑'),
                        _ => {}
                    }
                }
            }
            b'\n' | b'\r' => i += 1,
            _ => {
                // Plain text: copy a whole UTF-8 run (ProPresenter 7 may write
                // raw UTF-8) or fall back to cp1252 for a stray high byte.
                let start = i;
                while i < rtf.len() && !matches!(rtf[i], b'\\' | b'{' | b'}' | b'\n' | b'\r') {
                    i += 1;
                }
                if hidden {
                    continue;
                }
                let run = &rtf[start..i];
                match std::str::from_utf8(run) {
                    Ok(s) => out.push_str(s),
                    Err(_) => out.extend(run.iter().map(|&b| cp1252(b))),
                }
            }
        }
    }
    clean(&out)
}

/// If a `\uN` low surrogate follows (after the `\uc` fallback characters),
/// returns it and the index after it.
fn peek_low_surrogate(rtf: &[u8], mut i: usize, uc: usize) -> Option<(u32, usize)> {
    i += uc; // fallback characters for the high surrogate
    if !rtf[i..].starts_with(b"\\u") {
        return None;
    }
    let mut j = i + 2;
    let neg = rtf.get(j) == Some(&b'-');
    if neg {
        j += 1;
    }
    let start = j;
    while j < rtf.len() && rtf[j].is_ascii_digit() {
        j += 1;
    }
    let n: i32 = std::str::from_utf8(&rtf[start..j]).ok()?.parse().ok()?;
    let unit = u32::from((if neg { -n } else { n }) as u16);
    if !(0xDC00..0xE000).contains(&unit) {
        return None;
    }
    if rtf.get(j) == Some(&b' ') {
        j += 1;
    }
    Some((unit, j + uc))
}

/// Trims line ends and the blank lines around the text; inner blank lines stay.
fn clean(s: &str) -> String {
    let s = s.replace(['\u{2028}', '\u{2029}', '\u{b}'], "\n");
    let lines: Vec<&str> = s.lines().map(str::trim_end).collect();
    let start = lines.iter().position(|l| !l.is_empty()).unwrap_or(0);
    let end = lines
        .iter()
        .rposition(|l| !l.is_empty())
        .map_or(0, |e| e + 1);
    lines[start..end.max(start)].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rtf_basics() {
        let rtf = br"{\rtf1\ansi\ansicpg1252{\fonttbl\f0\fswiss Helvetica;}{\colortbl;\red255\green0\blue0;}\f0\fs48 Amazing grace\par how sweet\'92s the sound\line caf\'e9 \u8212? end}";
        assert_eq!(
            rtf_to_text(rtf),
            "Amazing grace\nhow sweet’s the sound\ncafé — end"
        );
    }

    #[test]
    fn rtf_backslash_newline_is_a_break_and_optional_destination_is_skipped() {
        let rtf = b"{\\rtf1{\\*\\generator Cocoa;}One\\\nTwo\\\\ \\{x\\}}";
        assert_eq!(rtf_to_text(rtf), "One\nTwo\\ {x}");
    }

    #[test]
    fn rtf_surrogate_pair() {
        let rtf = br"{\rtf1 \uc1\u-10179?\u-8704?}";
        assert_eq!(rtf_to_text(rtf), "😀");
    }
}
