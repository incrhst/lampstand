//! OpenLyrics XML (<https://docs.openlyrics.org>), versions 0.8 and 0.9.
//!
//! One file holds one song. OpenLP exports its library in this format.

use roxmltree::{Document, Node};

use lampstand_core::song::{Section, Song};

use crate::{finish, parse_arrangement, ImportError, Imported, Report};

pub fn parse(text: &str) -> Result<Imported, ImportError> {
    let doc = Document::parse(text.trim_start_matches('\u{feff}'))?;
    let root = doc.root_element();
    if root.tag_name().name() != "song" {
        return Err(ImportError::WrongFormat { expected: "OpenLyrics" });
    }

    let mut report = Report::default();
    let mut song = Song::default();
    let mut ignored: Vec<&str> = Vec::new();

    if let Some(props) = child(root, "properties") {
        for prop in props.children().filter(Node::is_element) {
            match prop.tag_name().name() {
                "titles" => {
                    for title in children(prop, "title") {
                        let t = collapse(&text_of(title));
                        if t.is_empty() {
                            continue;
                        }
                        if song.title.is_empty() {
                            song.title = t;
                        } else {
                            song.alternate_titles.push(t);
                        }
                    }
                }
                "authors" => {
                    for author in children(prop, "author") {
                        let name = collapse(&text_of(author));
                        if !name.is_empty() && !song.authors.contains(&name) {
                            song.authors.push(name);
                        }
                    }
                }
                "copyright" => song.copyright = non_empty(collapse(&text_of(prop))),
                "ccliNo" => {
                    let raw = collapse(&text_of(prop));
                    match raw.parse() {
                        Ok(n) => song.ccli_number = Some(n),
                        Err(_) if raw.is_empty() => {}
                        Err(_) => ignored.push("CCLI number (not a number)"),
                    }
                }
                "key" => song.key = non_empty(collapse(&text_of(prop))),
                "tempo" => {
                    let raw = collapse(&text_of(prop));
                    match raw.parse() {
                        Ok(bpm) if prop.attribute("type") != Some("text") => song.tempo_bpm = Some(bpm),
                        _ if raw.is_empty() => {}
                        _ => ignored.push("tempo (only beats per minute are kept)"),
                    }
                }
                "verseOrder" => song.arrangement = parse_arrangement(&text_of(prop)),
                "comments" => ignored.push("comments"),
                "songbooks" => ignored.push("songbook entries"),
                "themes" => ignored.push("themes"),
                "variant" => ignored.push("variant"),
                "publisher" => ignored.push("publisher"),
                "keywords" => ignored.push("keywords"),
                "released" => ignored.push("release date"),
                "transposition" => ignored.push("transposition"),
                "timeSignature" => ignored.push("time signature"),
                _ => {}
            }
        }
    }

    let mut main_lang: Option<Option<&str>> = None;
    if let Some(lyrics) = child(root, "lyrics") {
        for item in lyrics.children().filter(Node::is_element) {
            match item.tag_name().name() {
                "verse" => {
                    let lang = item.attribute("lang");
                    let main = *main_lang.get_or_insert(lang);
                    if item.attribute("translit").is_some() || lang != main {
                        let what = match (item.attribute("translit"), lang) {
                            (Some(t), _) => format!("transliteration ({t})"),
                            (None, Some(l)) => format!("lyrics in language {l:?}"),
                            (None, None) => "lyrics with no language".into(),
                        };
                        report.note(
                            Some(&song.title),
                            format!("{what} not imported; one language per song for now (SL-5)"),
                        );
                        continue;
                    }
                    let name = item.attribute("name").unwrap_or("v");
                    let mut lines = Vec::new();
                    for block in children(item, "lines") {
                        read_lines(block, &mut lines, &mut ignored);
                    }
                    let section = Section::new(name, lines);
                    if song.sections.iter().any(|s| s.label == section.label) {
                        ignored.push("repeated section names (kept the first)");
                    } else {
                        song.sections.push(section);
                    }
                }
                "instrument" => ignored.push("instrumental parts"),
                _ => {}
            }
        }
    }
    if child(root, "format").is_some() {
        ignored.push("formatting tag definitions");
    }

    for what in ignored {
        report.note(Some(&song.title), format!("{what} not imported"));
    }
    let song = finish(song, &mut report);
    Ok(Imported { songs: vec![song], report })
}

/// Reads one `<lines>` element into lyric lines with inline chords. Handles
/// both the 0.8 form (`<line>` children) and the 0.9 form (`<br/>`).
fn read_lines(block: Node, out: &mut Vec<String>, ignored: &mut Vec<&str>) {
    let mut current = String::new();
    walk(block, &mut current, out, ignored);
    // Text after the last <br/> or </line>, if any is left.
    if !current.trim().is_empty() {
        push_line(&mut current, out);
    }
}

fn walk(node: Node, current: &mut String, out: &mut Vec<String>, ignored: &mut Vec<&str>) {
    for n in node.children() {
        if n.is_text() {
            current.push_str(n.text().unwrap_or(""));
            continue;
        }
        if !n.is_element() {
            continue;
        }
        match n.tag_name().name() {
            "br" => push_line(current, out),
            "line" => {
                walk(n, current, out, ignored);
                push_line(current, out);
            }
            "chord" => {
                if let Some(chord) = chord_name(n) {
                    current.push('[');
                    current.push_str(&chord);
                    current.push(']');
                }
                walk(n, current, out, ignored);
            }
            "tag" => {
                ignored.push("text formatting");
                walk(n, current, out, ignored);
            }
            "comment" => ignored.push("comments within lyrics"),
            _ => walk(n, current, out, ignored),
        }
    }
}

fn push_line(current: &mut String, out: &mut Vec<String>) {
    let line = collapse(current);
    current.clear();
    out.push(line);
}

/// `name` in 0.8; `root`, `structure` and `bass` in 0.9.
fn chord_name(node: Node) -> Option<String> {
    if let Some(name) = node.attribute("name") {
        return Some(name.to_string());
    }
    let root = node.attribute("root")?;
    let structure = match node.attribute("structure").unwrap_or("") {
        "" | "maj" => "",
        "min" => "m",
        "dom7" => "7",
        "min7" => "m7",
        other => other,
    };
    let bass = node.attribute("bass").map(|b| format!("/{b}")).unwrap_or_default();
    Some(format!("{root}{structure}{bass}"))
}

fn child<'a, 'i>(node: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    node.children().find(|n| n.is_element() && n.tag_name().name() == name)
}

fn children<'a, 'i: 'a>(node: Node<'a, 'i>, name: &'a str) -> impl Iterator<Item = Node<'a, 'i>> + 'a {
    node.children().filter(move |n| n.is_element() && n.tag_name().name() == name)
}

fn text_of(node: Node) -> String {
    node.descendants().filter(Node::is_text).filter_map(|n| n.text()).collect()
}

/// Collapses runs of whitespace, including the newlines of pretty-printed
/// XML, to single spaces.
fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn non_empty(s: String) -> Option<String> {
    (!s.is_empty()).then_some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_09_chords_with_root_and_structure() {
        let xml = r#"<song xmlns="http://openlyrics.info/namespace/2009/song" version="0.9">
            <properties><titles><title>T</title></titles></properties>
            <lyrics><verse name="v1"><lines><chord root="G"/>Hello <chord root="D" structure="dom7" bass="F#"/>world<br/>again</lines></verse></lyrics>
        </song>"#;
        let song = &parse(xml).unwrap().songs[0];
        assert_eq!(song.sections[0].lines, ["[G]Hello [D7/F#]world", "again"]);
    }

    #[test]
    fn rejects_other_xml() {
        assert!(matches!(parse("<foo/>"), Err(ImportError::WrongFormat { .. })));
    }
}
