//! The live engine: what the operator has on preview and on air, and how
//! each key press moves it. A pure state machine with no I/O, no clock of
//! its own and no fonts, so every behaviour is a unit test.
//!
//! PRD: LV-1, LV-2, PS-8, RP-1.
//!
//! The app loads a service as `LoadedItem`s (songs and scripture already
//! split into slides, decks already parsed), sends `Command`s from the
//! operator view, clickers and remotes, and forwards the resulting
//! `Program` to the compositor and the `Event`s to logging and the UI.
//!
//! Rules (each one has a test below):
//! - `Next` advances a deck's build step, then the slide, then moves on to
//!   the first slide of the next item that has slides. `Previous` mirrors
//!   it, landing on the last slide of the previous item at its final step.
//! - `Next` with nothing live takes the preview cursor live, or the first
//!   slide of the service if nothing is previewed.
//! - Black and Logo are toggles that persist across slide changes. Clear
//!   text and clear background last until the next slide goes live.
//! - Reloading the service (an edit synced in mid-service) never changes
//!   what is on air: the live item keeps the slides it was loaded with
//!   until the operator leaves it. Cursors follow items by ID, so a reorder
//!   keeps the operator in place.
//! - `SongShown` fires when a song item goes live from another item, once
//!   per visit, for CCLI reporting.

use lampstand_core::id::Timestamp;
use lampstand_core::program::{Content, Program};
use lampstand_core::service::ItemId;
use lampstand_core::song::SongId;
use lampstand_core::theme::{Background, ThemeId};
use serde::{Deserialize, Serialize};

/// One service item, ready to play.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedItem {
    pub id: ItemId,
    pub title: String,
    /// Empty for headers, which navigation skips.
    pub slides: Vec<LoadedSlide>,
    pub background: Option<Background>,
    pub theme: Option<ThemeId>,
    /// Set for song items, for `SongShown`.
    pub song: Option<SongId>,
    /// Announcement loops: advance by themselves every this many
    /// milliseconds, wrapping to the first slide (driven by `tick`).
    pub auto_advance_ms: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoadedSlide {
    pub content: Content,
    /// Build steps (clicks) after the slide appears. 0 for text slides.
    pub steps: usize,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Cursor {
    pub item: ItemId,
    pub slide: usize,
    pub step: usize,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Command {
    /// Put a slide on the preview pane.
    Preview(Cursor),
    /// Take the preview live.
    GoLive,
    /// Take a slide live directly (clicking a slide in the operator view).
    Show(Cursor),
    Next,
    Previous,
    ToggleBlack,
    ToggleLogo,
    ClearText,
    ClearBackground,
    /// Text, background and overlays off: a clean screen.
    ClearAll,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    ProgramChanged,
    PreviewChanged,
    ItemLive {
        item: ItemId,
    },
    /// A build step advanced on a deck slide: the compositor plays that
    /// step's animations instead of cutting to the new state.
    BuildStep {
        cursor: Cursor,
    },
    SongShown {
        song: SongId,
    },
}

#[derive(Debug, Default)]
pub struct LiveEngine {
    items: Vec<LoadedItem>,
    live: Option<Cursor>,
    live_item: Option<LoadedItem>,
    preview: Option<Cursor>,
    program: Program,
}

impl LiveEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads or reloads the service. See the module rules for what happens
    /// to the live and preview cursors.
    pub fn load(&mut self, items: Vec<LoadedItem>) -> Vec<Event> {
        let _ = items;
        todo!("T33")
    }

    pub fn apply(&mut self, command: Command) -> Vec<Event> {
        let _ = command;
        todo!("T33")
    }

    /// Drives auto-advancing items and countdowns. Call at least every
    /// 100 ms with the current time.
    pub fn tick(&mut self, now: Timestamp) -> Vec<Event> {
        let _ = now;
        todo!("T34")
    }

    pub fn program(&self) -> &Program {
        &self.program
    }

    pub fn live(&self) -> Option<Cursor> {
        self.live
    }

    pub fn preview(&self) -> Option<Cursor> {
        self.preview
    }

    /// Nothing on air. The updater only installs when this is true
    /// (CD-11).
    pub fn is_idle(&self) -> bool {
        let _ = (&self.items, &self.live_item);
        self.program.content.is_none()
            && self.program.overlay == lampstand_core::program::Overlay::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lampstand_core::program::{Overlay, SlideText};
    use std::sync::Arc;

    fn text(s: &str) -> LoadedSlide {
        LoadedSlide {
            content: Content::Text {
                slide: SlideText {
                    label: None,
                    lines: vec![s.into()],
                    footer: None,
                },
            },
            steps: 0,
        }
    }

    fn deck_slide(steps: usize) -> LoadedSlide {
        let scene = Arc::new(lampstand_core::scene::Scene {
            width: 960.0,
            height: 540.0,
            nodes: vec![],
        });
        LoadedSlide {
            content: Content::DeckSlide {
                deck: lampstand_core::deck::DeckId::generate(),
                index: 0,
                step: 0,
                scene,
                notes: String::new(),
            },
            steps,
        }
    }

    fn item(title: &str, slides: Vec<LoadedSlide>) -> LoadedItem {
        LoadedItem {
            id: ItemId::generate(),
            title: title.into(),
            slides,
            background: Some(Background::Color(lampstand_core::geom::Color::rgb(
                0, 0, 80,
            ))),
            theme: None,
            song: None,
            auto_advance_ms: None,
        }
    }

    fn on_air(engine: &LiveEngine) -> Option<String> {
        match &engine.program().content {
            Some(Content::Text { slide }) => Some(slide.lines[0].clone()),
            Some(Content::DeckSlide { step, .. }) => Some(format!("deck step {step}")),
            None => None,
            other => panic!("unexpected {other:?}"),
        }
    }

    fn service() -> (LiveEngine, Vec<LoadedItem>) {
        let items = vec![
            item("Welcome", vec![text("a1"), text("a2")]),
            item("Header", vec![]),
            item("Song", vec![text("b1")]),
        ];
        let mut engine = LiveEngine::new();
        engine.load(items.clone());
        (engine, items)
    }

    #[test]
    #[ignore = "T33"]
    fn next_walks_slides_and_skips_headers() {
        let (mut e, _) = service();
        assert_eq!(on_air(&e), None);
        let seen: Vec<_> = (0..4)
            .map(|_| {
                e.apply(Command::Next);
                on_air(&e)
            })
            .collect();
        assert_eq!(
            seen,
            [
                Some("a1".into()),
                Some("a2".into()),
                Some("b1".into()),
                Some("b1".into())
            ]
        );
        e.apply(Command::Previous);
        assert_eq!(on_air(&e).as_deref(), Some("a2"));
    }

    #[test]
    #[ignore = "T33"]
    fn next_takes_preview_live_first() {
        let (mut e, items) = service();
        e.apply(Command::Preview(Cursor {
            item: items[2].id,
            slide: 0,
            step: 0,
        }));
        assert_eq!(on_air(&e), None);
        e.apply(Command::Next);
        assert_eq!(on_air(&e).as_deref(), Some("b1"));
    }

    #[test]
    #[ignore = "T33"]
    fn builds_advance_before_slides() {
        let deck = item("Sermon", vec![deck_slide(2), deck_slide(0)]);
        let mut e = LiveEngine::new();
        e.load(vec![deck.clone()]);
        e.apply(Command::Next);
        assert_eq!(on_air(&e).as_deref(), Some("deck step 0"));
        let events = e.apply(Command::Next);
        assert!(events.contains(&Event::BuildStep {
            cursor: Cursor {
                item: deck.id,
                slide: 0,
                step: 1
            }
        }));
        e.apply(Command::Next);
        assert_eq!(
            e.live(),
            Some(Cursor {
                item: deck.id,
                slide: 0,
                step: 2
            })
        );
        e.apply(Command::Next);
        assert_eq!(
            e.live(),
            Some(Cursor {
                item: deck.id,
                slide: 1,
                step: 0
            })
        );
        e.apply(Command::Previous);
        assert_eq!(
            e.live(),
            Some(Cursor {
                item: deck.id,
                slide: 0,
                step: 2
            })
        );
    }

    #[test]
    #[ignore = "T33"]
    fn black_persists_and_clear_text_does_not() {
        let (mut e, _) = service();
        e.apply(Command::Next);
        e.apply(Command::ToggleBlack);
        e.apply(Command::Next);
        assert_eq!(e.program().overlay, Overlay::Black);
        assert_eq!(on_air(&e).as_deref(), Some("a2"));
        e.apply(Command::ToggleBlack);
        assert_eq!(e.program().overlay, Overlay::None);

        e.apply(Command::ClearText);
        assert_eq!(on_air(&e), None);
        assert!(e.program().background.is_some());
        e.apply(Command::Next);
        assert_eq!(on_air(&e).as_deref(), Some("b1"));
    }

    #[test]
    #[ignore = "T33"]
    fn clear_all_goes_idle() {
        let (mut e, _) = service();
        e.apply(Command::Next);
        e.apply(Command::ToggleLogo);
        assert!(!e.is_idle());
        e.apply(Command::ClearAll);
        assert!(e.is_idle());
        assert_eq!(e.program().background, None);
    }

    #[test]
    #[ignore = "T33"]
    fn reload_keeps_live_item_and_follows_reorder() {
        let (mut e, mut items) = service();
        e.apply(Command::Next);
        // A planner edits the live item's text and moves it to the end.
        let mut edited = items.remove(0);
        edited.slides = vec![text("changed")];
        items.push(edited.clone());
        e.load(items);
        assert_eq!(on_air(&e).as_deref(), Some("a1"));
        e.apply(Command::Next);
        assert_eq!(
            on_air(&e).as_deref(),
            Some("a2"),
            "old slides until the item is left"
        );
        e.apply(Command::Next);
        assert_eq!(
            on_air(&e).as_deref(),
            Some("a2"),
            "it is now the last item, so Next stays put"
        );
        e.apply(Command::Show(Cursor {
            item: edited.id,
            slide: 0,
            step: 0,
        }));
        assert_eq!(
            on_air(&e).as_deref(),
            Some("a1"),
            "same item: still the live snapshot"
        );
        let song = e.items[1].id;
        e.apply(Command::Show(Cursor {
            item: song,
            slide: 0,
            step: 0,
        }));
        e.apply(Command::Show(Cursor {
            item: edited.id,
            slide: 0,
            step: 0,
        }));
        assert_eq!(
            on_air(&e).as_deref(),
            Some("changed"),
            "left and came back: new slides"
        );
    }

    #[test]
    #[ignore = "T33"]
    fn song_shown_once_per_visit() {
        let song = SongId::generate();
        let mut s = item("Song", vec![text("v1"), text("c")]);
        s.song = Some(song);
        let other = item("Prayer", vec![text("p")]);
        let mut e = LiveEngine::new();
        e.load(vec![s, other]);
        let shown = |events: Vec<Event>| {
            events
                .iter()
                .filter(|ev| matches!(ev, Event::SongShown { .. }))
                .count()
        };
        assert_eq!(shown(e.apply(Command::Next)), 1);
        assert_eq!(shown(e.apply(Command::Next)), 0);
        assert_eq!(shown(e.apply(Command::Next)), 0);
        assert_eq!(shown(e.apply(Command::Previous)), 1);
    }

    #[test]
    #[ignore = "T33"]
    fn next_on_stage_shows_following_slide() {
        let (mut e, _) = service();
        e.apply(Command::Next);
        e.apply(Command::Next);
        match &e.program().next {
            Some(Content::Text { slide }) => assert_eq!(slide.lines[0], "b1"),
            other => panic!("{other:?}"),
        }
    }
}
