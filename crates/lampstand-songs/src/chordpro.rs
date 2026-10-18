//! ChordPro text (<https://www.chordpro.org/chordpro/>).
//!
//! Lyric lines are already in Lampstand's storage form, so they are kept
//! as written. Sections come from `{start_of_verse}`-style environments, or,
//! in older files with none, from blank-line separated paragraphs, using a
//! preceding `{comment: Chorus}` as the label when there is one.
//! `{chorus}` repeats the last chorus in the arrangement, and `{new_song}`
//! starts the next song in the same file.

use lampstand_core::song::{Section, SectionKind, Song};

use crate::{finish, Imported, Report};

/// ChordPro text is always readable, so this never fails; anything it does
/// not understand goes in the report.
pub fn parse(text: &str) -> Imported {
    let mut imported = Imported::default();
    let mut parser = Parser::default();
    for line in text.trim_start_matches('\u{feff}').lines() {
        let trimmed = line.trim();
        if let Some((name, value)) = directive(trimmed) {
            if matches!(name.as_str(), "new_song" | "ns") {
                parser.finish_into(&mut imported);
                parser = Parser::default();
            } else {
                parser.directive(&name, value);
            }
        } else if trimmed.starts_with('#') {
            continue;
        } else {
            parser.lyric(line.trim_end());
        }
    }
    parser.finish_into(&mut imported);
    imported
}

#[derive(Default)]
struct Parser {
    song: Song,
    report: Report,
    /// Lines of the section being read.
    lines: Vec<String>,
    /// Label for the current section, from an environment or a comment.
    label: Option<String>,
    in_environment: bool,
    /// Inside `{start_of_tab}` or `{start_of_grid}`: skip lines.
    skipping: bool,
    /// Sung order, built as sections and `{chorus}` repeats appear.
    order: Vec<String>,
    repeated: bool,
    has_content: bool,
}

impl Parser {
    fn title(&self) -> Option<&str> {
        (!self.song.title.is_empty()).then_some(self.song.title.as_str())
    }

    fn note(&mut self, message: String) {
        let title = self.song.title.clone();
        self.report.note((!title.is_empty()).then_some(title.as_str()), message);
    }

    fn directive(&mut self, name: &str, value: &str) {
        self.has_content = true;
        let value = value.trim();
        match name {
            "title" | "t" => self.song.title = value.to_string(),
            "subtitle" | "st" | "sorttitle" => {
                if !value.is_empty() {
                    self.song.alternate_titles.push(value.to_string())
                }
            }
            "artist" | "composer" | "lyricist" | "author" => {
                if !value.is_empty() && !self.song.authors.iter().any(|a| a == value) {
                    self.song.authors.push(value.to_string())
                }
            }
            "copyright" | "(c)" => self.song.copyright = Some(value.to_string()),
            "ccli" | "ccli_song_number" => match value.parse() {
                Ok(n) => self.song.ccli_number = Some(n),
                Err(_) => self.note(format!("CCLI number {value:?} is not a number; not imported")),
            },
            "key" => self.song.key = Some(value.to_string()),
            "tempo" => match value.parse() {
                Ok(n) => self.song.tempo_bpm = Some(n),
                Err(_) => self.note(format!("tempo {value:?} not imported (only beats per minute are kept)")),
            },
            "comment" | "c" | "comment_italic" | "ci" | "comment_box" | "cb" | "highlight" => {
                // Old files mark sections with a comment such as "Chorus".
                let (kind, _) = SectionKind::parse_label(value);
                if kind != SectionKind::Other && !self.in_environment {
                    self.end_section();
                    self.label = Some(value.to_string());
                } else {
                    self.note("comments not imported".into());
                }
            }
            "chorus" => {
                self.end_section();
                let label = if value.is_empty() {
                    self.song.sections.iter().rev().find(|s| s.kind == SectionKind::Chorus).map(|s| s.label.clone())
                } else {
                    Some(SectionKind::parse_label(value).1)
                };
                match label {
                    Some(l) => {
                        self.order.push(l);
                        self.repeated = true;
                    }
                    None => self.note("{chorus} with no chorus before it; skipped".into()),
                }
            }
            _ => {
                if let Some(env) = name.strip_prefix("start_of_").or_else(|| short_start(name)) {
                    self.start_environment(env, value);
                } else if name.starts_with("end_of_") || matches!(name, "eov" | "eoc" | "eob" | "eot" | "eog") {
                    if self.skipping {
                        self.skipping = false;
                    } else {
                        self.end_section();
                        self.in_environment = false;
                    }
                } else {
                    self.note(format!("{{{name}}} not imported"));
                }
            }
        }
    }

    fn start_environment(&mut self, env: &str, value: &str) {
        self.end_section();
        let label_attr = value.strip_prefix("label=").map(|v| v.trim_matches('"')).unwrap_or(value).trim();
        let default = match env {
            "verse" => "Verse",
            "chorus" => "Chorus",
            "bridge" => "Bridge",
            "tab" | "grid" | "abc" | "ly" | "svg" | "textblock" => {
                self.note(format!("{env} blocks not imported"));
                self.skipping = true;
                return;
            }
            other => other,
        };
        self.label = Some(if label_attr.is_empty() { default.to_string() } else { label_attr.to_string() });
        self.in_environment = true;
    }

    fn lyric(&mut self, line: &str) {
        if self.skipping {
            return;
        }
        if line.trim().is_empty() {
            if !self.in_environment {
                self.end_section();
            } else if !self.lines.is_empty() {
                // Keep a blank line inside an environment as a stanza gap.
                self.lines.push(String::new());
            }
            return;
        }
        self.has_content = true;
        self.lines.push(line.to_string());
    }

    fn end_section(&mut self) {
        while self.lines.last().is_some_and(|l| l.is_empty()) {
            self.lines.pop();
        }
        if self.lines.is_empty() {
            return;
        }
        let lines = std::mem::take(&mut self.lines);
        let label = self.label.take().unwrap_or_else(|| "Verse".into());
        let mut section = Section::new(&label, lines);
        // "Verse" with no number: number it by position, as V1, V2...
        if section.kind == SectionKind::Verse && section.label == "V" {
            let n = self.song.sections.iter().filter(|s| s.kind == SectionKind::Verse).count() + 1;
            section.label = format!("V{n}");
        }
        // Keep labels unique: a second unnumbered chorus becomes C2.
        if self.song.sections.iter().any(|s| s.label == section.label) {
            let base = section.label.clone();
            let mut n = 2;
            while self.song.sections.iter().any(|s| s.label == format!("{base}{n}")) {
                n += 1;
            }
            section.label = format!("{base}{n}");
        }
        self.order.push(section.label.clone());
        self.song.sections.push(section);
    }

    fn finish_into(mut self, imported: &mut Imported) {
        self.end_section();
        if !self.has_content {
            return;
        }
        if self.repeated {
            self.song.arrangement = std::mem::take(&mut self.order);
        }
        if self.title().is_none() && self.song.sections.is_empty() {
            return;
        }
        let song = finish(self.song, &mut self.report);
        imported.songs.push(song);
        imported.report.extend(self.report);
    }
}

fn short_start(name: &str) -> Option<&'static str> {
    Some(match name {
        "sov" => "verse",
        "soc" => "chorus",
        "sob" => "bridge",
        "sot" => "tab",
        "sog" => "grid",
        _ => return None,
    })
}

/// Splits `{name: value}` or `{name value}` into a lower-case name and value.
fn directive(line: &str) -> Option<(String, &str)> {
    let inner = line.strip_prefix('{')?.strip_suffix('}')?;
    let split = inner.find(|c: char| c == ':' || c.is_whitespace()).unwrap_or(inner.len());
    let (name, rest) = inner.split_at(split);
    let value = rest.strip_prefix(':').unwrap_or(rest);
    // Conditional directives such as {title-en: ...} keep their base name.
    let name = name.split('-').next().unwrap_or(name).to_ascii_lowercase();
    Some((name, value.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_directive_forms() {
        assert_eq!(directive("{title: Hi}"), Some(("title".into(), "Hi")));
        assert_eq!(directive("{soc}"), Some(("soc".into(), "")));
        assert_eq!(directive("{start_of_verse label=\"Verse 2\"}"), Some(("start_of_verse".into(), "label=\"Verse 2\"")));
        assert_eq!(directive("[G]not a directive"), None);
    }

    #[test]
    fn paragraphs_and_comment_labels_make_sections() {
        let text = "{t: Song}\n\n[G]One\n\n{c: Chorus}\nSing\n\nTwo\n{chorus}\n";
        let imported = parse(text);
        let song = &imported.songs[0];
        let labels: Vec<_> = song.sections.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(labels, ["V1", "C", "V2"]);
        assert_eq!(song.arrangement, ["V1", "C", "V2", "C"]);
        assert!(imported.report.is_empty(), "{:?}", imported.report);
    }

    #[test]
    fn new_song_splits_files() {
        let imported = parse("{t: A}\nx\n{ns}\n{t: B}\ny\n");
        let titles: Vec<_> = imported.songs.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(titles, ["A", "B"]);
    }
}
