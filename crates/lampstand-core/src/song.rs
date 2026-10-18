//! Songs: sections, arrangement order and licensing details.
//!
//! PRD: SL-1.

/// A labelled block of lyrics, such as "V1" or "C".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub label: String,
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Song {
    pub title: String,
    pub author: Option<String>,
    pub copyright: Option<String>,
    pub ccli_number: Option<u32>,
    pub key: Option<String>,
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
    /// The sections in sung order, repeats included.
    pub fn arranged(&self) -> Result<Vec<&Section>, SongError> {
        if self.arrangement.is_empty() {
            return Ok(self.sections.iter().collect());
        }
        self.arrangement
            .iter()
            .map(|label| {
                self.sections
                    .iter()
                    .find(|s| &s.label == label)
                    .ok_or_else(|| SongError::UnknownSection(label.clone()))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(label: &str) -> Section {
        Section { label: label.into(), lines: vec![format!("{label} line")] }
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
}
