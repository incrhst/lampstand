//! OpenSong song files (<http://www.opensong.org/home/file-formats>).
//!
//! One XML file per song, usually with no extension. The lyrics are a
//! plain-text block: `[V]` starts a section, lines starting with `.` hold
//! chords placed above the next lyric line, a leading digit numbers the
//! verse a line belongs to (so one `[V]` block can hold verses 1 to 3), and
//! `;` starts a comment.

use roxmltree::{Document, Node};

use lampstand_core::song::{Section, Song};

use crate::{finish, parse_arrangement, ImportError, Imported, Report};

pub fn parse(text: &str) -> Result<Imported, ImportError> {
    let doc = Document::parse(text.trim_start_matches('\u{feff}'))?;
    let root = doc.root_element();
    if root.tag_name().name() != "song" || field(root, "lyrics").is_none() && field(root, "title").is_none() {
        return Err(ImportError::WrongFormat { expected: "OpenSong" });
    }

    let mut report = Report::default();
    let mut song = Song { title: field(root, "title").unwrap_or_default(), ..Default::default() };
    let title = song.title.clone();
    let title = Some(title.as_str());

    if let Some(author) = field(root, "author") {
        song.authors.push(author);
    }
    song.copyright = field(root, "copyright");
    if let Some(ccli) = field(root, "ccli") {
        match ccli.parse() {
            Ok(n) => song.ccli_number = Some(n),
            Err(_) => report.note(title, format!("CCLI number {ccli:?} is not a number; not imported")),
        }
    }
    song.key = field(root, "key");
    if let Some(tempo) = field(root, "tempo") {
        match tempo.parse() {
            Ok(bpm) => song.tempo_bpm = Some(bpm),
            Err(_) => report.note(title, format!("tempo {tempo:?} not imported (only beats per minute are kept)")),
        }
    }
    if let Some(aka) = field(root, "aka") {
        song.alternate_titles.push(aka);
    }
    for (tag, what) in [
        ("hymn_number", "hymn number"),
        ("capo", "capo"),
        ("time_sig", "time signature"),
        ("theme", "theme"),
        ("alttheme", "alternate theme"),
        ("user1", "user field 1"),
        ("user2", "user field 2"),
        ("user3", "user field 3"),
        ("key_line", "key line"),
        ("linked_songs", "linked songs"),
    ] {
        if field(root, tag).is_some() {
            report.note(title, format!("{what} not imported"));
        }
    }
    for (tag, what) in [("style", "per-song style"), ("backgrounds", "backgrounds")] {
        if root.children().any(|n| n.tag_name().name() == tag && n.has_children()) {
            report.note(title, format!("{what} not imported"));
        }
    }

    song.sections = parse_lyrics(&field(root, "lyrics").unwrap_or_default(), title, &mut report);
    if let Some(order) = field(root, "presentation") {
        song.arrangement = parse_arrangement(&order);
    }

    let song = finish(song, &mut report);
    Ok(Imported { songs: vec![song], report })
}

/// A `[X]` block. Numbered lines go to `numbered[n]`, the rest to `plain`.
struct Block {
    label: String,
    plain: Vec<String>,
    numbered: Vec<(char, Vec<String>)>,
}

fn parse_lyrics(text: &str, title: Option<&str>, report: &mut Report) -> Vec<Section> {
    let mut blocks: Vec<Block> = Vec::new();
    let mut chords: Option<&str> = None;

    for raw in text.lines() {
        let raw = raw.trim_end();
        let Some(first) = raw.chars().next() else { continue };
        match first {
            '[' => {
                let label = raw.trim_start_matches('[').split(']').next().unwrap_or("").trim();
                blocks.push(Block { label: label.to_string(), plain: Vec::new(), numbered: Vec::new() });
                chords = None;
            }
            '.' => chords = Some(&raw[1..]),
            ';' => {
                report.note(title, "comments within lyrics not imported");
                chords = None;
            }
            _ => {
                let (number, body) = match first {
                    '1'..='9' => (Some(first), &raw[1..]),
                    ' ' => (None, &raw[1..]),
                    _ => (None, raw),
                };
                let merged = match chords {
                    Some(c) => merge_chords(c, body),
                    None => body.to_string(),
                };
                // A chord line covers every numbered line under it, but only
                // the one plain line that follows.
                if number.is_none() {
                    chords = None;
                }
                if merged.contains("||") {
                    report.note(title, "forced slide breaks (||) not imported");
                }
                let lines = merged
                    .split('|')
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty());

                if blocks.is_empty() {
                    blocks.push(Block { label: "V".into(), plain: Vec::new(), numbered: Vec::new() });
                }
                let block = blocks.last_mut().expect("just pushed");
                match number {
                    None => block.plain.extend(lines),
                    Some(n) => match block.numbered.iter_mut().find(|(m, _)| *m == n) {
                        Some((_, v)) => v.extend(lines),
                        None => block.numbered.push((n, lines.collect())),
                    },
                }
            }
        }
    }

    let mut sections: Vec<Section> = Vec::new();
    for mut block in blocks {
        if !block.plain.is_empty() || block.numbered.is_empty() {
            sections.push(Section::new(&block.label, block.plain));
        }
        block.numbered.sort_by_key(|(n, _)| *n);
        for (n, lines) in block.numbered {
            sections.push(Section::new(&format!("{}{n}", block.label), lines));
        }
    }
    sections
}

/// Places the chords from an OpenSong chord line into the lyric line below
/// it, by column, as ChordPro inline chords.
fn merge_chords(chord_line: &str, lyric: &str) -> String {
    let mut chords: Vec<(usize, &str)> = Vec::new();
    let mut start = None;
    for (col, (i, c)) in chord_line.char_indices().enumerate() {
        if c.is_whitespace() {
            if let Some((s_col, s_byte)) = start.take() {
                chords.push((s_col, &chord_line[s_byte..i]));
            }
        } else if start.is_none() {
            start = Some((col, i));
        }
    }
    if let Some((s_col, s_byte)) = start {
        chords.push((s_col, &chord_line[s_byte..]));
    }

    let chars: Vec<char> = lyric.chars().collect();
    let mut out = String::new();
    let mut next = 0;
    for (col, chord) in chords {
        // Chords past the end of the words are kept, one space apart.
        let col = col.min(chars.len());
        out.extend(&chars[next..col]);
        if col == chars.len() && col > 0 && !out.ends_with(' ') {
            out.push(' ');
        }
        out.push('[');
        out.push_str(chord);
        out.push(']');
        next = col;
    }
    out.extend(&chars[next..]);
    out.trim_end().to_string()
}

/// The trimmed text of a direct child element, or None if missing or empty.
fn field(root: Node, name: &str) -> Option<String> {
    let node = root.children().find(|n| n.is_element() && n.tag_name().name() == name)?;
    let text: String = node.descendants().filter(Node::is_text).filter_map(|n| n.text()).collect();
    let trimmed = if name == "lyrics" { text.trim_matches('\n') } else { text.trim() };
    (!trimmed.trim().is_empty()).then(|| trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_chords_by_column() {
        assert_eq!(merge_chords("G      D", "Amazing grace"), "[G]Amazing[D] grace");
        assert_eq!(merge_chords("G         D/F#", "Hi"), "[G]Hi [D/F#]");
    }

    #[test]
    fn numbered_lines_become_separate_verses() {
        let mut report = Report::default();
        let sections = parse_lyrics("[V]\n.G\n1One\n2Two\n[C]\n Chorus", None, &mut report);
        let labels: Vec<_> = sections.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(labels, ["V1", "V2", "C"]);
        assert_eq!(sections[1].lines, ["[G]Two"]);
        assert_eq!(sections[2].lines, ["Chorus"]);
    }
}
