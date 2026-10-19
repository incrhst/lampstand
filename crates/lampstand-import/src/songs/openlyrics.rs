//! OpenLyrics 0.9 XML, one song per file or a folder of them. Also used by OpenLP's exports and by T06.

use std::path::Path;

use lampstand_core::song::{Section, Song};
use quick_xml::escape::resolve_predefined_entity;
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};

use crate::{ImportError, Skipped, SongImport};

/// Imports one OpenLyrics file, or every `.xml` file directly inside a folder.
/// A file that cannot be read goes to `skipped`; only an unreadable `path`
/// fails the import.
pub fn import(path: &Path) -> Result<SongImport, ImportError> {
    let mut out = SongImport::default();
    let files = if path.is_dir() {
        let mut files: Vec<_> = std::fs::read_dir(path)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("xml")))
            .collect();
        files.sort();
        files
    } else {
        std::fs::metadata(path)?;
        vec![path.to_path_buf()]
    };
    for file in files {
        let source = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let parsed = std::fs::read(&file)
            .map_err(|e| e.to_string())
            .and_then(|bytes| String::from_utf8(bytes).map_err(|_| "not valid UTF-8".to_string()))
            .and_then(|xml| parse_song(&xml));
        match parsed {
            Ok(song) => out.songs.push(song),
            Err(reason) => out.skipped.push(Skipped { source, reason }),
        }
    }
    Ok(out)
}

/// Parses one OpenLyrics `<song>` document. Chords become inline ChordPro
/// (`[G]`), verse names become labels (`v1` → `V1`) and `verseOrder`
/// becomes the arrangement.
pub fn parse_song(xml: &str) -> Result<Song, String> {
    let mut reader = Reader::from_str(xml);
    let mut stack: Vec<String> = Vec::new();
    let mut seen_root = false;

    let mut titles: Vec<String> = Vec::new();
    let mut authors: Vec<String> = Vec::new();
    let mut copyright = String::new();
    let mut ccli = String::new();
    let mut verse_order = String::new();
    let mut key = String::new();
    let mut tempo = String::new();
    let mut prop = String::new();

    let mut sections: Vec<Section> = Vec::new();
    // (label, lang, lines) of the verse being read.
    let mut verse: Option<(String, bool, Vec<String>)> = None;
    let mut line = String::new();

    loop {
        let event = reader.read_event().map_err(|e| format!("bad XML: {e}"))?;
        let empty = matches!(event, Event::Empty(_));
        match event {
            Event::Start(ref e) | Event::Empty(ref e) => {
                let name = local(e);
                if !seen_root {
                    if name != "song" {
                        return Err(format!("not an OpenLyrics song (root is <{name}>)"));
                    }
                    seen_root = true;
                }
                match name.as_str() {
                    "verse" if !empty => {
                        let label = attr(e, "name").unwrap_or_default().to_uppercase();
                        verse = Some((label, attr(e, "lang").is_some(), Vec::new()));
                    }
                    "lines" if !empty => line.clear(),
                    "br" if verse.is_some() => flush_line(&mut line, &mut verse),
                    "chord" if verse.is_some() && !in_comment(&stack) => {
                        if let Some(c) = attr(e, "name") {
                            line.push_str(&format!("[{c}]"));
                        }
                    }
                    _ => {}
                }
                if !empty {
                    stack.push(name);
                    prop.clear();
                }
            }
            Event::End(_) => {
                match stack.pop().as_deref() {
                    Some("lines") => flush_line(&mut line, &mut verse),
                    Some("verse") => {
                        if let Some((label, has_lang, lines)) = verse.take() {
                            commit_verse(&mut sections, label, has_lang, lines);
                        }
                    }
                    Some("title") => titles.push(take_text(&mut prop)),
                    Some("author") => authors.push(take_text(&mut prop)),
                    Some("copyright") => copyright = take_text(&mut prop),
                    Some("ccliNo") => ccli = take_text(&mut prop),
                    Some("verseOrder") => verse_order = take_text(&mut prop),
                    Some("key") => key = take_text(&mut prop),
                    Some("tempo") => tempo = take_text(&mut prop),
                    _ => {}
                }
                prop.clear();
            }
            Event::Text(t) => {
                let text = t.xml_content(XmlVersion::Implicit1_0);
                add_text(&text, &stack, verse.is_some(), &mut line, &mut prop);
            }
            Event::CData(t) => {
                let text = t.xml_content(XmlVersion::Implicit1_0);
                add_text(&text, &stack, verse.is_some(), &mut line, &mut prop);
            }
            Event::GeneralRef(r) => {
                let name = r.xml_content(XmlVersion::Implicit1_0);
                let text = if let Ok(Some(c)) = r.resolve_char_ref() {
                    c.to_string()
                } else if let Some(s) = resolve_predefined_entity(&name) {
                    s.to_string()
                } else {
                    return Err(format!("unknown entity &{name};"));
                };
                add_text(&text, &stack, verse.is_some(), &mut line, &mut prop);
            }
            Event::Eof => break,
            _ => {}
        }
    }

    if !seen_root {
        return Err("not an OpenLyrics song (empty document)".into());
    }
    if !stack.is_empty() {
        return Err("bad XML: unclosed element".into());
    }
    let title = titles
        .into_iter()
        .find(|t| !t.is_empty())
        .ok_or("song has no title")?;
    if sections.is_empty() {
        return Err("song has no lyrics".into());
    }

    let mut song = Song::new(title);
    let authors: Vec<_> = authors.into_iter().filter(|a| !a.is_empty()).collect();
    song.author = (!authors.is_empty()).then(|| authors.join(", "));
    song.copyright = non_empty(copyright);
    song.ccli_number = ccli.trim().parse().ok();
    song.key = non_empty(key);
    song.tempo = tempo.trim().parse().ok();
    song.arrangement = verse_order
        .split_whitespace()
        .map(str::to_uppercase)
        .filter(|label| sections.iter().any(|s| &s.label == label))
        .collect();
    song.sections = sections;
    Ok(song)
}

fn local(e: &BytesStart) -> String {
    e.local_name().as_ref().to_string()
}

fn attr(e: &BytesStart, name: &str) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.local_name().as_ref() == name)
        .and_then(|a| a.normalized_value(XmlVersion::Implicit1_0).ok())
        .map(|v| v.into_owned())
}

fn in_comment(stack: &[String]) -> bool {
    stack.iter().any(|n| n == "comment")
}

fn add_text(text: &str, stack: &[String], in_verse: bool, line: &mut String, prop: &mut String) {
    if in_verse {
        if stack.iter().any(|n| n == "lines") && !in_comment(stack) {
            line.push_str(text);
        }
    } else {
        prop.push_str(text);
    }
}

fn take_text(prop: &mut String) -> String {
    let text = std::mem::take(prop);
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn non_empty(s: String) -> Option<String> {
    (!s.is_empty()).then_some(s)
}

fn flush_line(line: &mut String, verse: &mut Option<(String, bool, Vec<String>)>) {
    let text = line.split_whitespace().collect::<Vec<_>>().join(" ");
    line.clear();
    if let (Some((_, _, lines)), false) = (verse.as_mut(), text.is_empty()) {
        lines.push(text);
    }
}

/// A verse written twice (a translation, marked with `lang`) keeps its first
/// text; the same name without `lang` continues the earlier section.
fn commit_verse(sections: &mut Vec<Section>, label: String, has_lang: bool, lines: Vec<String>) {
    if label.is_empty() || lines.is_empty() {
        return;
    }
    match sections.iter_mut().find(|s| s.label == label) {
        Some(existing) if !has_lang => existing.lines.extend(lines),
        Some(_) => {}
        None => sections.push(Section { label, lines }),
    }
}
