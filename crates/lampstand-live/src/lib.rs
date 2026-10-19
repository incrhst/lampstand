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
//! - If a synced edit deletes the live item, it stays on air, and `Next`
//!   and `Previous` carry on from where it stood: to the item that
//!   followed it by Position, or the one before it.
//! - `SongShown` fires when a song item goes live from another item, once
//!   per visit, for CCLI reporting.

use lampstand_core::id::Timestamp;
use lampstand_core::program::{Content, Overlay, Program};
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
    /// Set while the live item has been deleted by a reload: the index in
    /// `items` where the items that followed it start.
    orphan_slot: Option<usize>,
}

impl LiveEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads or reloads the service. See the module rules for what happens
    /// to the live and preview cursors.
    pub fn load(&mut self, items: Vec<LoadedItem>) -> Vec<Event> {
        if let Some(live) = self.live {
            self.orphan_slot = if items.iter().any(|i| i.id == live.item) {
                None
            } else {
                // How many surviving items stood before the live one.
                let before = self
                    .item_index(live.item)
                    .or(self.orphan_slot)
                    .unwrap_or(self.items.len());
                Some(
                    self.items[..before.min(self.items.len())]
                        .iter()
                        .filter(|old| items.iter().any(|i| i.id == old.id))
                        .count(),
                )
            };
        }
        self.items = items;
        let mut events = Vec::new();
        if let Some(preview) = self.preview {
            if self.resolve(preview).is_none() {
                self.preview = None;
                events.push(Event::PreviewChanged);
            }
        }
        // The stage display's "next" follows the new order; what is on air
        // does not change.
        if let Some(live) = self.live {
            let next = self.next_content(live);
            if next != self.program.next {
                self.program.next = next;
                events.push(Event::ProgramChanged);
            }
        }
        events
    }

    pub fn apply(&mut self, command: Command) -> Vec<Event> {
        let mut events = Vec::new();
        match command {
            Command::Preview(cursor) => {
                if let Some(cursor) = self.resolve(cursor) {
                    self.preview = Some(cursor);
                    events.push(Event::PreviewChanged);
                }
            }
            Command::GoLive => {
                if let Some(preview) = self.preview {
                    self.go_live(preview, &mut events);
                }
            }
            Command::Show(cursor) => self.go_live(cursor, &mut events),
            Command::Next => {
                let target = match self.live {
                    Some(live) => self.step_forward(live),
                    None => self
                        .preview
                        .and_then(|p| self.resolve(p))
                        .or_else(|| self.first_slide_from(0)),
                };
                if let Some(target) = target {
                    self.go_live(target, &mut events);
                }
            }
            Command::Previous => {
                if let Some(target) = self.live.and_then(|live| self.step_back(live)) {
                    self.go_live(target, &mut events);
                }
            }
            Command::ToggleBlack => self.toggle_overlay(Overlay::Black, &mut events),
            Command::ToggleLogo => self.toggle_overlay(Overlay::Logo, &mut events),
            Command::ClearText => {
                if self.program.content.take().is_some() {
                    events.push(Event::ProgramChanged);
                }
            }
            Command::ClearBackground => {
                if self.program.background.take().is_some() {
                    events.push(Event::ProgramChanged);
                }
            }
            Command::ClearAll => {
                let changed = self.program.content.is_some()
                    || self.program.background.is_some()
                    || self.program.overlay != Overlay::None;
                self.program.content = None;
                self.program.background = None;
                self.program.overlay = Overlay::None;
                if changed {
                    events.push(Event::ProgramChanged);
                }
            }
        }
        events
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

    /// Nothing on air and no service under way: no item has gone live, and
    /// there is no text, background or overlay on the output. The updater
    /// only installs when this is true (CD-11), so once a service starts it
    /// waits for the next launch.
    pub fn is_idle(&self) -> bool {
        self.live.is_none()
            && self.program.content.is_none()
            && self.program.background.is_none()
            && self.program.overlay == Overlay::None
    }
}

// Navigation helpers.
impl LiveEngine {
    /// The item a cursor refers to. The live item keeps the slides it was
    /// loaded with until the operator leaves it.
    fn item_for(&self, id: ItemId) -> Option<&LoadedItem> {
        match (&self.live_item, self.live) {
            (Some(item), Some(live)) if live.item == id => Some(item),
            _ => self.items.iter().find(|i| i.id == id),
        }
    }

    /// Checks a cursor against the slides it points at, clamping the step.
    fn resolve(&self, cursor: Cursor) -> Option<Cursor> {
        let slide = self.item_for(cursor.item)?.slides.get(cursor.slide)?;
        Some(Cursor {
            step: cursor.step.min(slide.steps),
            ..cursor
        })
    }

    fn steps_at(&self, item: ItemId, slide: usize) -> usize {
        self.item_for(item)
            .and_then(|i| i.slides.get(slide))
            .map_or(0, |s| s.steps)
    }

    fn content_at(&self, cursor: Cursor) -> Option<Content> {
        let slide = self.item_for(cursor.item)?.slides.get(cursor.slide)?;
        let mut content = slide.content.clone();
        if let Content::DeckSlide { step, .. } = &mut content {
            *step = cursor.step;
        }
        Some(content)
    }

    /// The first slide of the first item at or after `from` that has slides.
    fn first_slide_from(&self, from: usize) -> Option<Cursor> {
        self.items
            .iter()
            .skip(from)
            .find(|i| !i.slides.is_empty())
            .map(|i| Cursor {
                item: i.id,
                slide: 0,
                step: 0,
            })
    }

    fn item_index(&self, id: ItemId) -> Option<usize> {
        self.items.iter().position(|i| i.id == id)
    }

    /// Where the items before `id` end and the items after it start, in
    /// `items`. For a live item a reload deleted, both are the slot it
    /// stood in.
    fn slot_of(&self, id: ItemId) -> Option<(usize, usize)> {
        if let Some(index) = self.item_index(id) {
            return Some((index, index + 1));
        }
        match (self.live, self.orphan_slot) {
            (Some(live), Some(slot)) if live.item == id => Some((slot, slot)),
            _ => None,
        }
    }

    /// The cursor `Next` would move to from `from`, or `None` at the end.
    fn step_forward(&self, from: Cursor) -> Option<Cursor> {
        let item = self.item_for(from.item)?;
        if from.step < self.steps_at(from.item, from.slide) {
            return Some(Cursor {
                step: from.step + 1,
                ..from
            });
        }
        if from.slide + 1 < item.slides.len() {
            return Some(Cursor {
                slide: from.slide + 1,
                step: 0,
                ..from
            });
        }
        self.first_slide_from(self.slot_of(from.item)?.1)
    }

    /// The cursor `Previous` would move to from `from`, at the final step
    /// of the slide it lands on.
    fn step_back(&self, from: Cursor) -> Option<Cursor> {
        if from.step > 0 {
            return Some(Cursor {
                step: from.step - 1,
                ..from
            });
        }
        if from.slide > 0 {
            let slide = from.slide - 1;
            return Some(Cursor {
                slide,
                step: self.steps_at(from.item, slide),
                ..from
            });
        }
        let (index, _) = self.slot_of(from.item)?;
        let prev = self.items[..index]
            .iter()
            .rev()
            .find(|i| !i.slides.is_empty())?;
        let slide = prev.slides.len() - 1;
        Some(Cursor {
            item: prev.id,
            slide,
            step: prev.slides[slide].steps,
        })
    }

    /// What the stage display shows as next: the following slide, or the
    /// first slide of the following item.
    fn next_content(&self, live: Cursor) -> Option<Content> {
        let item = self.item_for(live.item)?;
        let cursor = if live.slide + 1 < item.slides.len() {
            Cursor {
                slide: live.slide + 1,
                step: 0,
                ..live
            }
        } else {
            self.first_slide_from(self.slot_of(live.item)?.1)?
        };
        self.content_at(cursor)
    }

    fn go_live(&mut self, cursor: Cursor, events: &mut Vec<Event>) {
        let Some(cursor) = self.resolve(cursor) else {
            return;
        };
        let previous = self.live;
        let same_item = previous.is_some_and(|p| p.item == cursor.item);
        let content = self.content_at(cursor);

        if !same_item {
            let item = self.items.iter().find(|i| i.id == cursor.item).cloned();
            // `resolve` found the item, and it was not the live one, so it
            // is in `items`.
            self.live_item = item;
            self.orphan_slot = None;
        }
        let Some(item) = &self.live_item else { return };
        self.program.background = item.background.clone();
        self.program.theme = item.theme;
        let song = item.song;
        self.program.content = content;
        self.live = Some(cursor);
        self.program.next = self.next_content(cursor);

        events.push(Event::ProgramChanged);
        if !same_item {
            events.push(Event::ItemLive { item: cursor.item });
            if let Some(song) = song {
                events.push(Event::SongShown { song });
            }
        }
        if same_item && previous.is_some_and(|p| p.slide == cursor.slide && p.step < cursor.step) {
            events.push(Event::BuildStep { cursor });
        }
    }

    fn toggle_overlay(&mut self, overlay: Overlay, events: &mut Vec<Event>) {
        self.program.overlay = if self.program.overlay == overlay {
            Overlay::None
        } else {
            overlay
        };
        events.push(Event::ProgramChanged);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lampstand_core::program::SlideText;
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
    fn clear_all_clears_the_screen_but_the_service_is_not_idle() {
        let (mut e, _) = service();
        assert!(e.is_idle());
        e.apply(Command::Next);
        e.apply(Command::ToggleLogo);
        assert!(!e.is_idle());
        e.apply(Command::ClearAll);
        assert_eq!(e.program().content, None);
        assert_eq!(e.program().background, None);
        assert_eq!(e.program().overlay, Overlay::None);
        assert!(!e.is_idle(), "an item is still live: no update mid-service");
    }

    #[test]
    fn clear_text_leaves_the_background_on_air_so_not_idle() {
        let (mut e, _) = service();
        e.apply(Command::Next);
        e.apply(Command::ClearText);
        assert_eq!(e.program().content, None);
        assert!(e.program().background.is_some());
        assert!(!e.is_idle());
    }

    #[test]
    fn deleted_live_item_still_navigates_by_position() {
        let items = vec![
            item("Welcome", vec![text("a1")]),
            item("Song", vec![text("b1"), text("b2")]),
            item("Prayer", vec![text("c1")]),
            item("Notices", vec![text("d1")]),
        ];
        let mut e = LiveEngine::new();
        e.load(items.clone());
        e.apply(Command::Show(Cursor {
            item: items[1].id,
            slide: 1,
            step: 0,
        }));
        // A planner deletes the live song.
        let without_song = vec![items[0].clone(), items[2].clone(), items[3].clone()];
        e.load(without_song.clone());
        assert_eq!(on_air(&e).as_deref(), Some("b2"), "on air is untouched");
        match &e.program().next {
            Some(Content::Text { slide }) => assert_eq!(slide.lines[0], "c1"),
            other => panic!("{other:?}"),
        }
        e.apply(Command::Next);
        assert_eq!(on_air(&e).as_deref(), Some("c1"));

        // Previous from the deleted item's first slide goes to the one before.
        e.load(items.clone());
        e.apply(Command::Show(Cursor {
            item: items[1].id,
            slide: 0,
            step: 0,
        }));
        e.load(without_song.clone());
        // A second reload (Prayer deleted too) keeps the slot.
        e.load(vec![items[0].clone(), items[3].clone()]);
        e.apply(Command::Previous);
        assert_eq!(on_air(&e).as_deref(), Some("a1"));

        e.load(items.clone());
        e.apply(Command::Show(Cursor {
            item: items[1].id,
            slide: 1,
            step: 0,
        }));
        e.load(without_song);
        e.load(vec![items[0].clone(), items[3].clone()]);
        e.apply(Command::Next);
        assert_eq!(on_air(&e).as_deref(), Some("d1"));
    }

    #[test]
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
    fn next_on_stage_shows_following_slide() {
        let (mut e, _) = service();
        e.apply(Command::Next);
        e.apply(Command::Next);
        match &e.program().next {
            Some(Content::Text { slide }) => assert_eq!(slide.lines[0], "b1"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn preview_and_go_live_on_headers_and_empty_service_do_nothing() {
        let (mut e, items) = service();
        let header = Cursor {
            item: items[1].id,
            slide: 0,
            step: 0,
        };
        assert!(e.apply(Command::Preview(header)).is_empty());
        assert_eq!(e.preview(), None);
        assert!(e.apply(Command::GoLive).is_empty());
        assert!(e.apply(Command::Show(header)).is_empty());
        assert_eq!(e.live(), None);
        assert!(e.is_idle());

        let mut empty = LiveEngine::new();
        empty.load(vec![]);
        for command in [Command::Next, Command::Previous, Command::GoLive] {
            assert!(empty.apply(command).is_empty());
        }
        assert_eq!(empty.live(), None);
        assert_eq!(empty.preview(), None);
    }

    #[test]
    fn show_on_missing_item_or_slide_does_nothing() {
        let (mut e, items) = service();
        let missing = Cursor {
            item: ItemId::generate(),
            slide: 0,
            step: 0,
        };
        assert!(e.apply(Command::Show(missing)).is_empty());
        assert!(e.apply(Command::Preview(missing)).is_empty());
        let past_end = Cursor {
            item: items[0].id,
            slide: 5,
            step: 0,
        };
        assert!(e.apply(Command::Show(past_end)).is_empty());
        assert_eq!(e.live(), None);
        assert!(e.is_idle());
    }
}
