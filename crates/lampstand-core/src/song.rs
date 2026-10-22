//! Songs: sections, arrangement order and licensing details.
//!
//! PRD: SL-1, SL-2 (chords are stored now so imports keep them; chord
//! sheets come in Phase 2).

use serde::{Deserialize, Serialize};

use crate::id::Id;

pub type SongId = Id<Song>;

/// A labelled block of lyrics, such as "V1" or "C".
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "automerge",
    derive(autosurgeon::Hydrate, autosurgeon::Reconcile)
)]
pub struct Section {
    pub label: String,
    /// One entry per displayed line, with chords inline in ChordPro form:
    /// `"[G]Amazing [G7]grace"`. Use [`lyrics`] to get the text to show.
    pub lines: Vec<String>,
}

impl Section {
    pub fn kind(&self) -> SectionKind {
        SectionKind::from_label(&self.label)
    }
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SectionKind {
    Verse,
    PreChorus,
    Chorus,
    Bridge,
    Tag,
    Intro,
    Ending,
    Other,
}

impl SectionKind {
    /// Reads the labels used by OpenSong, OpenLP and OpenLyrics: "V1", "C",
    /// "C2", "P", "B", "T", "I", "E", and their spelled-out forms ("Verse 2",
    /// "Pre-Chorus", "chorus"). Case-insensitive; anything else is `Other`.
    pub fn from_label(label: &str) -> Self {
        let lower = label.trim().to_lowercase();
        let word: String = lower
            .trim_end_matches(|c: char| c.is_ascii_digit() || c.is_whitespace())
            .chars()
            .filter(|c| !matches!(c, ' ' | '-' | '_'))
            .collect();
        match word.as_str() {
            "v" | "verse" => Self::Verse,
            "c" | "chorus" | "refrain" => Self::Chorus,
            "p" | "prechorus" => Self::PreChorus,
            "b" | "bridge" => Self::Bridge,
            "t" | "tag" => Self::Tag,
            "i" | "intro" => Self::Intro,
            "e" | "ending" | "outro" => Self::Ending,
            _ => Self::Other,
        }
    }
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "automerge",
    derive(autosurgeon::Hydrate, autosurgeon::Reconcile)
)]
pub struct Song {
    pub id: SongId,
    pub title: String,
    pub author: Option<String>,
    pub copyright: Option<String>,
    pub ccli_number: Option<u32>,
    pub key: Option<String>,
    pub tempo: Option<u16>,
    pub sections: Vec<Section>,
    /// Section labels in the order they are sung. Empty means the order the
    /// sections are written in.
    pub arrangement: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SongError {
    UnknownSection(String),
}

impl Song {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            id: SongId::generate(),
            title: title.into(),
            author: None,
            copyright: None,
            ccli_number: None,
            key: None,
            tempo: None,
            sections: Vec::new(),
            arrangement: Vec::new(),
        }
    }

    /// The sections in sung order, repeats included.
    pub fn arranged(&self) -> Result<Vec<&Section>, SongError> {
        self.arranged_by(&self.arrangement)
    }

    /// As [`Song::arranged`], with a per-service arrangement in place of the
    /// song's own.
    pub fn arranged_by(&self, arrangement: &[String]) -> Result<Vec<&Section>, SongError> {
        if arrangement.is_empty() {
            return Ok(self.sections.iter().collect());
        }
        arrangement
            .iter()
            .map(|label| {
                self.section(label)
                    .ok_or_else(|| SongError::UnknownSection(label.clone()))
            })
            .collect()
    }

    /// The section a label names: an exact match first, then ignoring case
    /// and surrounding space, so "v1" finds "V1".
    pub fn section(&self, label: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.label == label).or_else(|| {
            let want = label.trim();
            self.sections
                .iter()
                .find(|s| s.label.trim().eq_ignore_ascii_case(want))
        })
    }

    /// The labels in `arrangement` that name no section, in the order given.
    pub fn unknown_labels<'a>(&self, arrangement: &'a [String]) -> Vec<&'a str> {
        arrangement
            .iter()
            .map(String::as_str)
            .filter(|label| self.section(label).is_none())
            .collect()
    }
}

/// Reads a typed sung order such as `"v1 v1, c v2"`: labels separated by
/// spaces and/or commas. Case is kept; lookup ignores it.
pub fn parse_order(text: &str) -> Vec<String> {
    text.split(|c: char| c == ',' || c.is_whitespace())
        .filter(|label| !label.is_empty())
        .map(String::from)
        .collect()
}

/// A ChordPro line with its chords removed and spacing tidied:
/// `"[G]Amazing [G7]grace"` becomes `"Amazing grace"`. A chord inside a word
/// (`"gr[C]ace"`) joins the word back up.
pub fn lyrics(line: &str) -> String {
    split_chords(line).0
}

/// The chords of a ChordPro line, with the byte offset in [`lyrics`]'s
/// output where each one sits.
pub fn chords(line: &str) -> Vec<(usize, String)> {
    split_chords(line).1
}

/// Splits a ChordPro line into its lyrics and chords. Whitespace runs
/// collapse to one space and the ends are trimmed. An unclosed `[` and an
/// empty `[]` are not chords: the first stays as text, the second is dropped.
fn split_chords(line: &str) -> (String, Vec<(usize, String)>) {
    let mut text = String::new();
    let mut found = Vec::new();
    let mut pending_space = false;
    let mut rest = line;
    while let Some(c) = rest.chars().next() {
        if c == '[' {
            if let Some(end) = rest.find(']') {
                let chord = rest[1..end].trim();
                if !chord.is_empty() {
                    found.push((text.len() + usize::from(pending_space), chord.to_string()));
                }
                rest = &rest[end + 1..];
                continue;
            }
        }
        if c.is_whitespace() {
            pending_space = !text.is_empty();
        } else {
            if pending_space {
                text.push(' ');
                pending_space = false;
            }
            text.push(c);
        }
        rest = &rest[c.len_utf8()..];
    }
    for (pos, _) in &mut found {
        *pos = (*pos).min(text.len());
    }
    (text, found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(label: &str) -> Section {
        Section {
            label: label.into(),
            lines: vec![format!("{label} line")],
        }
    }

    fn song(arrangement: &[&str]) -> Song {
        Song {
            sections: vec![section("V1"), section("C"), section("V2")],
            arrangement: arrangement.iter().map(|s| s.to_string()).collect(),
            ..Song::new("Test")
        }
    }

    fn labels(song: &Song) -> Vec<String> {
        song.arranged()
            .unwrap()
            .iter()
            .map(|s| s.label.clone())
            .collect()
    }

    #[test]
    fn empty_arrangement_uses_written_order() {
        assert_eq!(labels(&song(&[])), ["V1", "C", "V2"]);
    }

    #[test]
    fn arrangement_repeats_sections() {
        assert_eq!(
            labels(&song(&["V1", "C", "V2", "C", "C"])),
            ["V1", "C", "V2", "C", "C"]
        );
    }

    #[test]
    fn unknown_label_is_an_error() {
        assert_eq!(
            song(&["V1", "B"]).arranged(),
            Err(SongError::UnknownSection("B".into()))
        );
    }

    #[test]
    fn typed_order_is_read_from_spaces_and_commas() {
        assert_eq!(parse_order("v1 v1 c, v2,v1"), ["v1", "v1", "c", "v2", "v1"]);
        assert_eq!(parse_order("  ,  "), Vec::<String>::new());
    }

    #[test]
    fn typed_order_repeats_and_omits_sections() {
        let mut s = song(&[]);
        s.sections.push(section("V3"));
        s.arrangement = parse_order("v1 v1 c v2 v1");
        assert_eq!(labels(&s), ["V1", "V1", "C", "V2", "V1"]);
    }

    #[test]
    fn labels_match_ignoring_case() {
        assert_eq!(labels(&song(&["v1", "c"])), ["V1", "C"]);
    }

    #[test]
    fn unknown_labels_are_listed() {
        let s = song(&[]);
        let order = parse_order("v1 b v9 c");
        assert_eq!(s.unknown_labels(&order), ["b", "v9"]);
    }

    #[test]
    fn section_kinds_from_labels() {
        use SectionKind::*;
        let cases = [
            ("V1", Verse),
            ("v", Verse),
            ("Verse 2", Verse),
            ("C", Chorus),
            ("C2", Chorus),
            ("Chorus", Chorus),
            ("P", PreChorus),
            ("Pre-Chorus", PreChorus),
            ("prechorus", PreChorus),
            ("B", Bridge),
            ("Bridge 2", Bridge),
            ("T", Tag),
            ("I", Intro),
            ("E", Ending),
            ("Outro", Ending),
            ("Vamp", Other),
        ];
        for (label, kind) in cases {
            assert_eq!(SectionKind::from_label(label), kind, "{label}");
        }
    }

    #[test]
    fn lyrics_strips_chords() {
        assert_eq!(
            lyrics("[G]Amazing [G7]grace, how [C]sweet"),
            "Amazing grace, how sweet"
        );
        assert_eq!(lyrics("A[D]maz - ing"), "Amaz - ing");
        assert_eq!(lyrics("gr[C]ace"), "grace");
        assert_eq!(lyrics("[G] [D]  Hallelujah"), "Hallelujah");
        assert_eq!(lyrics("No chords here"), "No chords here");
    }

    #[test]
    fn chords_keep_positions() {
        assert_eq!(
            chords("[G]Amazing [G7]grace"),
            [(0, "G".to_string()), (8, "G7".to_string())]
        );
        assert_eq!(chords("plain"), []);
    }

    #[test]
    fn section_kind_edge_cases() {
        use SectionKind::*;
        for (label, kind) in [
            ("", Other),
            ("  c2 ", Chorus),
            ("PRE CHORUS", PreChorus),
            ("pre_chorus 2", PreChorus),
            ("Refrain", Chorus),
            ("Ending", Ending),
            ("Intro 1", Intro),
            ("Verse2", Verse),
            ("1", Other),
        ] {
            assert_eq!(SectionKind::from_label(label), kind, "{label:?}");
        }
    }

    #[test]
    fn lyrics_edge_cases() {
        assert_eq!(lyrics(""), "");
        assert_eq!(lyrics("[G]"), "");
        assert_eq!(lyrics("  spaced   out  [C] "), "spaced out");
        assert_eq!(lyrics("broken [G chord"), "broken [G chord");
        assert_eq!(lyrics("empty[]brackets"), "emptybrackets");
        assert_eq!(lyrics("[Am]Grâce [F]à toi"), "Grâce à toi");
    }

    #[test]
    fn chord_positions_follow_tidied_text() {
        assert_eq!(
            chords("  [G]one   [D]two [C]"),
            [(0, "G".into()), (4, "D".into()), (7, "C".into())]
        );
        assert_eq!(chords("gr[C]ace"), [(2, "C".to_string())]);
        assert_eq!(
            chords("[G] [D]  Hallelujah"),
            [(0, "G".into()), (0, "D".into())]
        );
        assert_eq!(
            chords("[Am]Grâce [F]à"),
            [(0, "Am".into()), (7, "F".into())]
        );
    }
}
