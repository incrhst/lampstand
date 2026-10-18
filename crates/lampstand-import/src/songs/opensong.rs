//! OpenSong songs: XML files without extension in a `Songs` folder, lyrics in OpenSong's `[V1]` / `.`chord-line text format.
//!
//! `path` is one song file or a `Songs` folder, searched recursively. A file
//! that is not an OpenSong song goes to `skipped`; the rest still import.
//!
//! Lyrics format, by first character of each line:
//!
//! - `[V1]` starts a section.
//! - `.` is a chord line; each chord sits above the lyric line that follows
//!   and is merged into it as inline ChordPro at the same column.
//! - a space starts a lyric line.
//! - `1`, `2`, ... start a lyric line for that verse only; a section with
//!   such lines becomes one section per verse (`V` → `V1`, `V2`), and lines
//!   without a number belong to every verse.
//! - `;` starts a comment.
//! - an empty line or `---` is a slide break, kept as an empty line.

use std::fs;
use std::path::{Path, PathBuf};

use quick_xml::events::Event;
use quick_xml::Reader;

use lampstand_core::song::{Section, Song};

use crate::{ImportError, Skipped, SongImport};

pub fn import(path: &Path) -> Result<SongImport, ImportError> {
    let meta = fs::metadata(path)?;
    let mut files = Vec::new();
    if meta.is_dir() {
        collect_files(path, &mut files)?;
    } else {
        files.push(path.to_path_buf());
    }

    let mut out = SongImport::default();
    for file in files {
        let name = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        match fs::read(&file).map_err(|e| e.to_string()).and_then(|b| {
            let text = String::from_utf8(b).map_err(|_| "not valid UTF-8".to_string())?;
            parse_song(&text, &name)
        }) {
            Ok(song) => out.songs.push(song),
            Err(reason) => out.skipped.push(Skipped {
                source: name,
                reason,
            }),
        }
    }
    Ok(out)
}

fn collect_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), ImportError> {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .collect();
    entries.sort();
    for p in entries {
        let hidden = p
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with('.'));
        if hidden {
            continue;
        }
        if p.is_dir() {
            collect_files(&p, files)?;
        } else {
            files.push(p);
        }
    }
    Ok(())
}

/// The text of the `<song>` element's direct children, by tag name.
fn read_fields(xml: &str) -> Result<Vec<(String, String)>, String> {
    let mut reader = Reader::from_str(xml);
    let mut depth = 0usize;
    let mut saw_root = false;
    let mut fields: Vec<(String, String)> = Vec::new();
    loop {
        match reader
            .read_event()
            .map_err(|e| format!("invalid XML: {e}"))?
        {
            Event::Start(e) => {
                let name = e.local_name().as_ref().to_string();
                if depth == 0 {
                    if name != "song" {
                        return Err(format!("not an OpenSong song (root element <{name}>)"));
                    }
                    saw_root = true;
                } else if depth == 1 {
                    fields.push((name, String::new()));
                }
                depth += 1;
            }
            Event::Empty(e) => {
                let name = e.local_name().as_ref().to_string();
                if depth == 0 {
                    if name != "song" {
                        return Err(format!("not an OpenSong song (root element <{name}>)"));
                    }
                    saw_root = true;
                } else if depth == 1 {
                    fields.push((name, String::new()));
                }
            }
            Event::End(_) => depth = depth.saturating_sub(1),
            Event::Text(t) if depth == 2 => {
                let text = t.xml10_content();
                if let Some((_, v)) = fields.last_mut() {
                    v.push_str(&text);
                }
            }
            Event::GeneralRef(r) if depth == 2 => {
                let name = r.as_ref().to_string();
                let resolved = if let Some(c) = r.resolve_char_ref().ok().flatten() {
                    c.to_string()
                } else {
                    quick_xml::escape::resolve_predefined_entity(&name)
                        .map(str::to_string)
                        .unwrap_or_default()
                };
                if let Some((_, v)) = fields.last_mut() {
                    v.push_str(&resolved);
                }
            }
            Event::CData(c) if depth == 2 => {
                let text = c.xml10_content();
                if let Some((_, v)) = fields.last_mut() {
                    v.push_str(&text);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !saw_root {
        return Err("not an OpenSong song (no <song> element)".into());
    }
    Ok(fields)
}

fn parse_song(xml: &str, file_name: &str) -> Result<Song, String> {
    let fields = read_fields(xml.trim_start_matches('\u{feff}'))?;
    let get = |name: &str| -> Option<String> {
        fields
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };

    let lyrics = fields
        .iter()
        .find(|(n, _)| n == "lyrics")
        .map(|(_, v)| v.as_str())
        .unwrap_or("");
    let parsed = parse_lyrics(lyrics);
    if parsed.sections.is_empty() {
        return Err("no lyrics".into());
    }

    let mut song = Song::new(get("title").unwrap_or_else(|| file_name.to_string()));
    song.author = get("author");
    song.copyright = get("copyright");
    song.ccli_number = get("ccli").and_then(|v| v.parse().ok());
    song.key = get("key");
    song.tempo = get("tempo").and_then(|v| v.parse().ok());

    if let Some(presentation) = get("presentation") {
        for token in presentation.split_whitespace() {
            let matched: Vec<&str> = parsed
                .sections
                .iter()
                .filter(|s| s.label.eq_ignore_ascii_case(token))
                .map(|s| s.label.as_str())
                .take(1)
                .collect();
            if !matched.is_empty() {
                song.arrangement.push(matched[0].to_string());
            } else if let Some((_, expanded)) = parsed
                .expansions
                .iter()
                .find(|(base, _)| base.eq_ignore_ascii_case(token))
            {
                song.arrangement.extend(expanded.iter().cloned());
            }
        }
    }
    song.sections = parsed.sections;
    Ok(song)
}

struct Parsed {
    sections: Vec<Section>,
    /// Base label → labels it expanded into, for multi-verse sections.
    expansions: Vec<(String, Vec<String>)>,
}

/// One line of a section: the verse it belongs to (`None` = every verse).
struct Raw {
    verse: Option<u32>,
    text: String,
}

fn parse_lyrics(text: &str) -> Parsed {
    let mut blocks: Vec<(String, Vec<Raw>)> = Vec::new();
    let mut pending_chords: Option<Vec<(usize, String)>> = None;

    for line in text.replace("\r\n", "\n").replace('\r', "\n").split('\n') {
        let line = line.trim_end();
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') && trimmed.len() > 2 {
            flush_chords(&mut pending_chords, &mut blocks);
            blocks.push((trimmed[1..trimmed.len() - 1].trim().to_string(), Vec::new()));
            continue;
        }
        let mut chars = line.chars();
        let Some(first) = chars.next() else {
            flush_chords(&mut pending_chords, &mut blocks);
            push_raw(&mut blocks, None, String::new());
            continue;
        };
        let rest: String = chars.collect();
        match first {
            ';' => {}
            '.' => {
                flush_chords(&mut pending_chords, &mut blocks);
                pending_chords = Some(chord_columns(&rest));
            }
            _ if line == "---" => {
                flush_chords(&mut pending_chords, &mut blocks);
                push_raw(&mut blocks, None, String::new());
            }
            c => {
                let (verse, body) = match c.to_digit(10) {
                    Some(n) => (Some(n), rest),
                    None if c == ' ' => (None, rest),
                    None => (None, line.to_string()),
                };
                let body = match pending_chords.take() {
                    Some(chords) => merge_chords(&body, &chords),
                    None => body,
                };
                push_raw(&mut blocks, verse, body);
            }
        }
    }
    flush_chords(&mut pending_chords, &mut blocks);

    let mut sections = Vec::new();
    let mut expansions = Vec::new();
    for (label, mut raws) in blocks {
        while raws.last().is_some_and(|r| r.text.is_empty()) {
            raws.pop();
        }
        while raws.first().is_some_and(|r| r.text.is_empty()) {
            raws.remove(0);
        }
        if raws.is_empty() {
            continue;
        }
        let mut verses: Vec<u32> = raws.iter().filter_map(|r| r.verse).collect();
        verses.sort_unstable();
        verses.dedup();
        if verses.is_empty() {
            sections.push(Section {
                label,
                lines: raws.into_iter().map(|r| r.text).collect(),
            });
            continue;
        }
        let base = label
            .trim_end_matches(|c: char| c.is_ascii_digit())
            .to_string();
        let mut labels = Vec::new();
        for v in verses {
            let new_label = format!("{base}{v}");
            let lines = raws
                .iter()
                .filter(|r| r.verse.is_none() || r.verse == Some(v))
                .map(|r| r.text.clone())
                .collect();
            labels.push(new_label.clone());
            sections.push(Section {
                label: new_label,
                lines,
            });
        }
        expansions.push((label, labels));
    }
    Parsed {
        sections,
        expansions,
    }
}

fn push_raw(blocks: &mut Vec<(String, Vec<Raw>)>, verse: Option<u32>, text: String) {
    if blocks.is_empty() {
        blocks.push(("V".to_string(), Vec::new()));
    }
    blocks.last_mut().unwrap().1.push(Raw { verse, text });
}

/// A chord line with no lyric line under it becomes a line of its own.
fn flush_chords(pending: &mut Option<Vec<(usize, String)>>, blocks: &mut Vec<(String, Vec<Raw>)>) {
    if let Some(chords) = pending.take() {
        let line = chords
            .iter()
            .map(|(_, c)| format!("[{c}]"))
            .collect::<Vec<_>>()
            .join(" ");
        push_raw(blocks, None, line);
    }
}

/// Chords of a chord line (without its leading `.`) with the column of each
/// within the lyric text below it.
fn chord_columns(line: &str) -> Vec<(usize, String)> {
    let mut chords = Vec::new();
    let mut current = String::new();
    let mut start = 0;
    for (i, c) in line.chars().enumerate() {
        if c.is_whitespace() {
            if !current.is_empty() {
                chords.push((start, std::mem::take(&mut current)));
            }
        } else {
            if current.is_empty() {
                start = i;
            }
            current.push(c);
        }
    }
    if !current.is_empty() {
        chords.push((start, current));
    }
    chords
}

fn merge_chords(lyric: &str, chords: &[(usize, String)]) -> String {
    let chars: Vec<char> = lyric.chars().collect();
    let mut out = String::new();
    let mut pos = 0;
    for (col, chord) in chords {
        while pos < *col {
            out.push(chars.get(pos).copied().unwrap_or(' '));
            pos += 1;
        }
        out.push('[');
        out.push_str(chord);
        out.push(']');
    }
    out.extend(chars.iter().skip(pos));
    out
}
