//! Song search with typo tolerance (SL-3).
//!
//! An in-memory index rebuilt at startup and updated on change. Libraries
//! run to a few thousand songs, so this stays well under 10 ms a query
//! without an on-disk engine.
//!
//! Ranking: title matches above lyric matches; whole words above prefixes
//! above typo matches (edit distance 1 for words of 4 to 7 letters, 2 for
//! longer). Case, accents and punctuation are ignored ("Ō" matches "o",
//! "it's" matches "its").

use lampstand_core::song::{Song, SongId};

#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub song: SongId,
    pub title: String,
    /// The matching lyric line, if the match was in the lyrics.
    pub line: Option<String>,
    pub score: f32,
}

#[derive(Default)]
pub struct SongIndex {
    _private: (),
}

impl SongIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds or replaces a song.
    pub fn upsert(&mut self, song: &Song) {
        let _ = song;
        todo!("T15")
    }

    pub fn remove(&mut self, id: SongId) {
        let _ = id;
        todo!("T15")
    }

    /// Best matches first. Every word in the query must match.
    pub fn search(&self, query: &str, limit: usize) -> Vec<Hit> {
        let _ = (query, limit);
        todo!("T15")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lampstand_core::song::Section;

    fn song(title: &str, lines: &[&str]) -> Song {
        Song {
            sections: vec![Section {
                label: "V1".into(),
                lines: lines.iter().map(|l| l.to_string()).collect(),
            }],
            ..Song::new(title)
        }
    }

    fn index() -> SongIndex {
        let mut i = SongIndex::new();
        i.upsert(&song(
            "Amazing Grace",
            &[
                "[G]Amazing grace how sweet the sound",
                "That saved a wretch like me",
            ],
        ));
        i.upsert(&song(
            "How Great Thou Art",
            &["O Lord my God", "When I in awesome wonder"],
        ));
        i.upsert(&song(
            "Great Is Thy Faithfulness",
            &["Great is Thy faithfulness, O God my Father"],
        ));
        i.upsert(&song("It's Your Blood", &["Washed in the blood"]));
        i
    }

    fn titles(hits: Vec<Hit>) -> Vec<String> {
        hits.into_iter().map(|h| h.title).collect()
    }

    #[test]
    #[ignore = "T15"]
    fn finds_by_title_prefix_and_typo() {
        let i = index();
        assert_eq!(titles(i.search("amaz", 5)), ["Amazing Grace"]);
        assert_eq!(titles(i.search("amazng grase", 5)), ["Amazing Grace"]);
        assert_eq!(titles(i.search("its your", 5)), ["It's Your Blood"]);
    }

    #[test]
    #[ignore = "T15"]
    fn title_matches_rank_above_lyrics() {
        let i = index();
        let mut great = titles(i.search("great", 5));
        great.sort();
        assert_eq!(great, ["Great Is Thy Faithfulness", "How Great Thou Art"]);
        let hits = i.search("wretch", 5);
        assert_eq!(hits[0].title, "Amazing Grace");
        assert_eq!(hits[0].line.as_deref(), Some("That saved a wretch like me"));
    }

    #[test]
    #[ignore = "T15"]
    fn chords_are_not_indexed_and_removal_works() {
        let mut i = index();
        assert!(i.search("[G]", 5).is_empty());
        let id = i.search("amazing", 1)[0].song;
        i.remove(id);
        assert!(i.search("amazing", 5).is_empty());
    }
}
