//! What an output is showing: the slides of the live item, which one is up,
//! and whether the screen is blanked.
//!
//! PRD: LV-1, LV-2.

use lampstand_core::song::{Song, SongError};

/// One screenful of lyrics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slide {
    pub label: String,
    pub text: String,
}

/// Slides for a song, one per section in sung order.
pub fn song_slides(song: &Song) -> Result<Vec<Slide>, SongError> {
    Ok(song
        .arranged()?
        .into_iter()
        .map(|section| Slide {
            label: section.label.clone(),
            text: section.lines.join("\n"),
        })
        .collect())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Screen {
    /// The current slide over the background.
    #[default]
    Live,
    /// Background only, text cleared.
    Blank,
    /// Everything black.
    Black,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Live {
    slides: Vec<Slide>,
    index: usize,
    screen: Screen,
}

impl Live {
    pub fn new(slides: Vec<Slide>) -> Self {
        Live {
            slides,
            index: 0,
            screen: Screen::Live,
        }
    }

    /// The slide the output shows, or `None` when blanked, black or empty.
    pub fn visible(&self) -> Option<&Slide> {
        match self.screen {
            Screen::Live => self.slides.get(self.index),
            Screen::Blank | Screen::Black => None,
        }
    }

    pub fn current(&self) -> Option<&Slide> {
        self.slides.get(self.index)
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn len(&self) -> usize {
        self.slides.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slides.is_empty()
    }

    pub fn screen(&self) -> Screen {
        self.screen
    }

    /// Moves to the next slide, stopping at the last. A blanked screen stays
    /// blanked, so the operator can line up the right slide unseen.
    pub fn next(&mut self) {
        if self.index + 1 < self.slides.len() {
            self.index += 1;
        }
    }

    pub fn previous(&mut self) {
        self.index = self.index.saturating_sub(1);
    }

    /// Toggles black: pressing it again returns to the slide.
    pub fn toggle_black(&mut self) {
        self.toggle(Screen::Black);
    }

    /// Toggles clearing the text while keeping the background.
    pub fn toggle_blank(&mut self) {
        self.toggle(Screen::Blank);
    }

    fn toggle(&mut self, screen: Screen) {
        self.screen = if self.screen == screen {
            Screen::Live
        } else {
            screen
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lampstand_core::song::Section;

    fn slides(labels: &[&str]) -> Vec<Slide> {
        labels
            .iter()
            .map(|l| Slide {
                label: l.to_string(),
                text: format!("{l} text"),
            })
            .collect()
    }

    #[test]
    fn song_slides_follow_the_arrangement() {
        let song = Song {
            sections: vec![
                Section {
                    label: "V1".into(),
                    lines: vec!["a".into(), "b".into()],
                },
                Section {
                    label: "C".into(),
                    lines: vec!["c".into()],
                },
            ],
            arrangement: vec!["V1".into(), "C".into(), "C".into()],
            ..Default::default()
        };
        let labels: Vec<_> = song_slides(&song)
            .unwrap()
            .into_iter()
            .map(|s| s.label)
            .collect();
        assert_eq!(labels, ["V1", "C", "C"]);
        assert_eq!(song_slides(&song).unwrap()[0].text, "a\nb");
    }

    #[test]
    fn next_and_previous_stop_at_the_ends() {
        let mut live = Live::new(slides(&["V1", "C"]));
        live.previous();
        assert_eq!(live.index(), 0);
        live.next();
        live.next();
        assert_eq!(live.index(), 1);
    }

    #[test]
    fn black_toggles_and_hides_the_slide() {
        let mut live = Live::new(slides(&["V1"]));
        live.toggle_black();
        assert_eq!((live.screen(), live.visible()), (Screen::Black, None));
        live.toggle_black();
        assert_eq!(live.visible().unwrap().label, "V1");
    }

    #[test]
    fn blank_then_black_switches_straight_to_black() {
        let mut live = Live::new(slides(&["V1"]));
        live.toggle_blank();
        live.toggle_black();
        assert_eq!(live.screen(), Screen::Black);
    }

    #[test]
    fn navigating_while_blanked_stays_blanked() {
        let mut live = Live::new(slides(&["V1", "C"]));
        live.toggle_blank();
        live.next();
        assert_eq!(
            (live.screen(), live.current().unwrap().label.as_str()),
            (Screen::Blank, "C")
        );
    }

    #[test]
    fn empty_show_has_nothing_visible() {
        let mut live = Live::new(vec![]);
        live.next();
        assert!(live.visible().is_none());
    }
}
