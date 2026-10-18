//! Songs: sections, arrangement order and licensing details.
//!
//! PRD: SL-1, SL-2 (chord storage).
//!
//! Lyric lines are stored in ChordPro inline form: a chord sits in square
//! brackets just before the syllable it falls on, as in `[G]Amazing [D]grace`.
//! Anything that shows lyrics to a congregation should use
//! [`Section::lyrics`], which strips the chords out.

use serde::{Deserialize, Serialize};

/// What part a section plays in a song.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SectionKind {
    #[default]
    Verse,
    PreChorus,
    Chorus,
    Bridge,
    Tag,
    Intro,
    /// Outro, ending or coda.
    Ending,
    Other,
}

impl SectionKind {
    /// The short code used in labels and arrangement strings, as in "V1" or
    /// "C". These match the OpenSong and OpenLyrics conventions.
    pub fn code(self) -> &'static str {
        match self {
            SectionKind::Verse => "V",
            SectionKind::PreChorus => "P",
            SectionKind::Chorus => "C",
            SectionKind::Bridge => "B",
            SectionKind::Tag => "T",
            SectionKind::Intro => "I",
            SectionKind::Ending => "E",
            SectionKind::Other => "O",
        }
    }

    /// The name shown to operators, as in "Verse" or "Pre-chorus".
    pub fn name(self) -> &'static str {
        match self {
            SectionKind::Verse => "Verse",
            SectionKind::PreChorus => "Pre-chorus",
            SectionKind::Chorus => "Chorus",
            SectionKind::Bridge => "Bridge",
            SectionKind::Tag => "Tag",
            SectionKind::Intro => "Intro",
            SectionKind::Ending => "Ending",
            SectionKind::Other => "Other",
        }
    }

    /// Reads a section label in either short form ("V1", "c", "PC", "v2a")
    /// or long form ("Verse 1", "Chorus", "Pre-Chorus 2").
    ///
    /// Returns the kind and the canonical label, such as `(Verse, "V1")`.
    /// Unrecognised labels come back as [`SectionKind::Other`] with the
    /// label trimmed and otherwise unchanged.
    pub fn parse_label(label: &str) -> (SectionKind, String) {
        let trimmed = label.trim();
        let lower = trimmed.to_lowercase();

        const LONG: &[(&str, SectionKind)] = &[
            ("pre-chorus", SectionKind::PreChorus),
            ("pre chorus", SectionKind::PreChorus),
            ("prechorus", SectionKind::PreChorus),
            ("verse", SectionKind::Verse),
            ("chorus", SectionKind::Chorus),
            ("refrain", SectionKind::Chorus),
            ("bridge", SectionKind::Bridge),
            ("tag", SectionKind::Tag),
            ("intro", SectionKind::Intro),
            ("outro", SectionKind::Ending),
            ("ending", SectionKind::Ending),
            ("coda", SectionKind::Ending),
        ];
        for (name, kind) in LONG {
            if let Some(rest) = lower.strip_prefix(name) {
                let suffix = rest.trim();
                if suffix.chars().all(|c| c.is_ascii_alphanumeric()) {
                    return (*kind, format!("{}{}", kind.code(), suffix.to_uppercase()));
                }
            }
        }

        const SHORT: &[(&str, SectionKind)] = &[
            ("pc", SectionKind::PreChorus),
            ("v", SectionKind::Verse),
            ("p", SectionKind::PreChorus),
            ("c", SectionKind::Chorus),
            ("b", SectionKind::Bridge),
            ("t", SectionKind::Tag),
            ("i", SectionKind::Intro),
            ("e", SectionKind::Ending),
            ("o", SectionKind::Other),
        ];
        for (code, kind) in SHORT {
            if let Some(rest) = lower.strip_prefix(code) {
                // A number must follow straight away (an optional letter after
                // it, as OpenLyrics allows "v1a"), so words such as "coda" or
                // "instrumental" are not read as codes.
                let mut chars = rest.chars();
                let suffix_ok = match chars.next() {
                    None => true,
                    Some(c) if c.is_ascii_digit() => rest.chars().all(|c| c.is_ascii_alphanumeric()),
                    _ => false,
                };
                if suffix_ok {
                    return (*kind, format!("{}{}", kind.code(), rest.to_uppercase()));
                }
            }
        }

        (SectionKind::Other, trimmed.to_string())
    }
}

/// A labelled block of lyrics, such as "V1" or "C".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Section {
    /// Unique within the song; arrangement entries refer to it.
    pub label: String,
    #[serde(default)]
    pub kind: SectionKind,
    /// Lyric lines with chords inline in ChordPro form.
    pub lines: Vec<String>,
}

impl Section {
    /// A section whose label is normalised ("Verse 1" becomes "V1") and whose
    /// kind is read from the label.
    pub fn new(label: &str, lines: Vec<String>) -> Self {
        let (kind, label) = SectionKind::parse_label(label);
        Section { label, kind, lines }
    }

    /// The lines with chords removed, for display. Lines that held only
    /// chords are left out.
    pub fn lyrics(&self) -> Vec<String> {
        self.lines
            .iter()
            .filter_map(|line| {
                let plain = strip_chords(line);
                let plain = plain.trim_end();
                (!plain.trim().is_empty() || line.trim().is_empty()).then(|| plain.to_string())
            })
            .collect()
    }

    pub fn has_chords(&self) -> bool {
        self.lines.iter().any(|l| l.contains('['))
    }
}

/// Removes ChordPro inline chords (`[G]`, `[Am7/C]`) from a line.
pub fn strip_chords(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(open) = rest.find('[') {
        match rest[open..].find(']') {
            Some(close) => {
                out.push_str(&rest[..open]);
                rest = &rest[open + close + 1..];
            }
            None => break,
        }
    }
    out.push_str(rest);
    out
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Song {
    pub title: String,
    /// Other names the song is known by, such as a first line or a title in
    /// another language. Searched alongside the title.
    pub alternate_titles: Vec<String>,
    pub authors: Vec<String>,
    pub copyright: Option<String>,
    pub ccli_number: Option<u32>,
    /// Musical key as written, such as "G" or "Bbm".
    pub key: Option<String>,
    pub tempo_bpm: Option<u16>,
    pub sections: Vec<Section>,
    /// Section labels in the order they are sung. Empty means the order the
    /// sections are written in.
    pub arrangement: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SongError {
    UnknownSection(String),
}

impl std::fmt::Display for SongError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SongError::UnknownSection(label) => write!(f, "arrangement names unknown section {label:?}"),
        }
    }
}

impl std::error::Error for SongError {}

impl Song {
    /// The sections in sung order, repeats included.
    pub fn arranged(&self) -> Result<Vec<&Section>, SongError> {
        if self.arrangement.is_empty() {
            return Ok(self.sections.iter().collect());
        }
        self.arrangement
            .iter()
            .map(|label| self.section(label).ok_or_else(|| SongError::UnknownSection(label.clone())))
            .collect()
    }

    pub fn section(&self, label: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.label == label)
    }

    /// All lyrics, chords removed, in written order. Used for search and
    /// previews.
    pub fn lyrics_text(&self) -> String {
        let mut text = String::new();
        for section in &self.sections {
            for line in section.lyrics() {
                text.push_str(&line);
                text.push('\n');
            }
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(label: &str) -> Section {
        Section::new(label, vec![format!("{label} line")])
    }

    fn song(arrangement: &[&str]) -> Song {
        Song {
            title: "Test".into(),
            sections: vec![section("V1"), section("C"), section("V2")],
            arrangement: arrangement.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    fn labels(song: &Song) -> Vec<String> {
        song.arranged().unwrap().iter().map(|s| s.label.clone()).collect()
    }

    #[test]
    fn empty_arrangement_uses_written_order() {
        assert_eq!(labels(&song(&[])), ["V1", "C", "V2"]);
    }

    #[test]
    fn arrangement_repeats_sections() {
        assert_eq!(labels(&song(&["V1", "C", "V2", "C", "C"])), ["V1", "C", "V2", "C", "C"]);
    }

    #[test]
    fn unknown_label_is_an_error() {
        assert_eq!(song(&["V1", "B"]).arranged(), Err(SongError::UnknownSection("B".into())));
    }

    #[test]
    fn parses_short_and_long_labels() {
        use SectionKind::*;
        let cases = [
            ("V1", Verse, "V1"),
            ("v2a", Verse, "V2A"),
            ("c", Chorus, "C"),
            ("Chorus 2", Chorus, "C2"),
            ("Verse 1", Verse, "V1"),
            ("Pre-Chorus", PreChorus, "P"),
            ("PC", PreChorus, "P"),
            ("Bridge", Bridge, "B"),
            ("T", Tag, "T"),
            ("Outro", Ending, "E"),
            ("Coda", Ending, "E"),
            ("Instrumental", Other, "Instrumental"),
        ];
        for (input, kind, label) in cases {
            assert_eq!(SectionKind::parse_label(input), (kind, label.to_string()), "{input}");
        }
    }

    #[test]
    fn strips_inline_chords() {
        assert_eq!(strip_chords("[G]Amazing [D/F#]grace"), "Amazing grace");
        assert_eq!(strip_chords("no chords [here"), "no chords [here");
    }

    #[test]
    fn lyrics_drop_chord_only_lines() {
        let s = Section::new("V1", vec!["[G] [D]".into(), "[G]Amazing grace".into()]);
        assert_eq!(s.lyrics(), ["Amazing grace"]);
        assert!(s.has_chords());
    }

    #[test]
    fn round_trips_through_json() {
        let original = Song { ccli_number: Some(22025), tempo_bpm: Some(72), ..song(&["V1", "C"]) };
        let json = serde_json::to_string(&original).unwrap();
        assert_eq!(serde_json::from_str::<Song>(&json).unwrap(), original);
    }
}
