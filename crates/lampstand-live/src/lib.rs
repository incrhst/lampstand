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
//! - `tick` takes the time as an argument; the engine never reads a clock,
//!   so tests drive it with made-up timestamps. Call it at least every
//!   100 ms. It returns no events when nothing changed.
//! - An item with `auto_advance_ms` (an announcement loop) advances itself
//!   to the next slide each time that many milliseconds pass, and wraps from
//!   the last slide to the first. The interval is counted from the first
//!   tick after the slide went live, so a slide that came up by hand gets
//!   its full time. A late tick (the machine slept) advances once, not once
//!   per missed interval. Loops step by slide, never by build step, and a
//!   loop of one slide does nothing.
//! - Operator control wins over the loop. `Next`, `Previous`, `Show` and
//!   `GoLive` move as usual and restart the interval. `Next` at the last
//!   slide leaves the loop for the next item; only `tick` wraps. Black and
//!   Logo cover the output but the loop keeps running underneath, so
//!   lifting them shows the slide that is current. `ClearText` and
//!   `ClearAll` stop the loop (the operator asked for an empty screen)
//!   until something goes live again. `ClearBackground` does not stop it,
//!   and the next slide brings the item's background back, as for any slide.
//! - A countdown shows the time left until its `target`, an absolute
//!   instant, so a countdown to a time of day needs no time zone here: the
//!   service item resolves the time of day to a `Timestamp`, and the
//!   compositor (given a UTC offset) formats it. The engine's part is to
//!   report `ProgramChanged` whenever the whole seconds on show change, on
//!   the live slide or in the stage display's "next". At zero it reports
//!   once, then holds `0:00` with no further events until the operator moves
//!   on. Under Black it reports nothing; lifting Black recomposes.
//! - A running loop or countdown is on air, so `is_idle` is false.
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
    /// The first slide of the next item that has slides, from wherever the
    /// live slide is (Shift+Down). With nothing live, the first item.
    NextItem,
    /// The first slide of the previous item that has slides (Shift+Up).
    /// Nothing happens at the start, or with nothing live.
    PreviousItem,
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
    /// The time passed to the latest `tick`.
    last_now: Option<Timestamp>,
    /// When the live loop's current slide started its interval. `None`
    /// until the first tick after it went live.
    loop_anchor: Option<Timestamp>,
    /// The operator cleared the screen: the loop waits for the next
    /// slide to go live.
    loop_halted: bool,
    /// Whole seconds left on the live and "next" countdowns as last
    /// reported, to tell when the display would change.
    countdown_shown: Option<CountdownKey>,
}

/// Seconds shown by the live slide and by the stage display's next slide.
type CountdownKey = (Option<i64>, Option<i64>);

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
            Command::NextItem => {
                let target = match self.live {
                    Some(live) => self
                        .slot_of(live.item)
                        .and_then(|(_, after)| self.first_slide_from(after)),
                    None => self.first_slide_from(0),
                };
                if let Some(target) = target {
                    self.go_live(target, &mut events);
                }
            }
            Command::PreviousItem => {
                let target = self.live.and_then(|live| {
                    let (before, _) = self.slot_of(live.item)?;
                    self.items[..before]
                        .iter()
                        .rev()
                        .find(|i| !i.slides.is_empty())
                        .map(|i| Cursor {
                            item: i.id,
                            slide: 0,
                            step: 0,
                        })
                });
                if let Some(target) = target {
                    self.go_live(target, &mut events);
                }
            }
            Command::ToggleBlack => self.toggle_overlay(Overlay::Black, &mut events),
            Command::ToggleLogo => self.toggle_overlay(Overlay::Logo, &mut events),
            Command::ClearText => {
                self.loop_halted = true;
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
                self.loop_halted = true;
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
        self.last_now = Some(now);
        let mut events = Vec::new();
        self.tick_loop(now, &mut events);
        self.tick_countdown(now, &mut events);
        events
    }

    fn tick_loop(&mut self, now: Timestamp, events: &mut Vec<Event>) {
        let (Some(live), false) = (self.live, self.loop_halted) else {
            return;
        };
        let Some(item) = &self.live_item else { return };
        let Some(interval) = item.auto_advance_ms.filter(|ms| *ms > 0) else {
            return;
        };
        let slides = item.slides.len();
        if slides < 2 {
            return;
        }
        // A clock that went backwards starts the interval again.
        let anchor = match self.loop_anchor {
            Some(anchor) if anchor <= now => anchor,
            _ => {
                self.loop_anchor = Some(now);
                return;
            }
        };
        let interval = i64::from(interval);
        let elapsed = now.0 - anchor.0;
        if elapsed < interval {
            return;
        }
        let next = Cursor {
            item: live.item,
            slide: (live.slide + 1) % slides,
            step: 0,
        };
        self.go_live(next, events);
        // Keep the cadence unless badly late, then start over from now.
        self.loop_anchor = Some(if elapsed >= 2 * interval {
            now
        } else {
            Timestamp(anchor.0 + interval)
        });
    }

    fn tick_countdown(&mut self, now: Timestamp, events: &mut Vec<Event>) {
        let key = self.countdown_key(now);
        let previous = self.countdown_shown.replace(key);
        if key == (None, None) || previous == Some(key) || self.program.overlay == Overlay::Black {
            return;
        }
        events.push(Event::ProgramChanged);
    }

    fn countdown_key(&self, now: Timestamp) -> CountdownKey {
        // Rounds up, like the compositor's formatting, so both agree on
        // which second is showing.
        let secs = |content: &Option<Content>| match content {
            Some(Content::Countdown { target, .. }) => {
                Some(((target.0 - now.0).max(0) + 999) / 1000)
            }
            _ => None,
        };
        (secs(&self.program.content), secs(&self.program.next))
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

        // A fresh slide gets a full interval and a countdown baseline.
        self.loop_anchor = None;
        self.loop_halted = false;
        self.countdown_shown = self.last_now.map(|now| self.countdown_key(now));

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
    fn next_item_jumps_to_the_start_of_the_next_item_from_any_slide() {
        let (mut e, items) = service();
        e.apply(Command::Show(Cursor {
            item: items[0].id,
            slide: 1,
            step: 0,
        }));
        assert_eq!(on_air(&e).as_deref(), Some("a2"));
        // The header between them has no slides, so it is skipped.
        e.apply(Command::NextItem);
        assert_eq!(on_air(&e).as_deref(), Some("b1"));
        // Past the last item nothing happens.
        e.apply(Command::NextItem);
        assert_eq!(on_air(&e).as_deref(), Some("b1"));
    }

    #[test]
    fn previous_item_goes_to_the_previous_items_first_slide_not_this_ones() {
        let (mut e, items) = service();
        e.apply(Command::Show(Cursor {
            item: items[2].id,
            slide: 0,
            step: 0,
        }));
        e.apply(Command::PreviousItem);
        assert_eq!(on_air(&e).as_deref(), Some("a1"));
        // Before the first item nothing happens.
        e.apply(Command::PreviousItem);
        assert_eq!(on_air(&e).as_deref(), Some("a1"));
    }

    #[test]
    fn with_nothing_live_next_item_starts_the_service_and_previous_does_nothing() {
        let (mut e, _) = service();
        assert!(e.apply(Command::PreviousItem).is_empty());
        assert_eq!(on_air(&e), None);
        e.apply(Command::NextItem);
        assert_eq!(on_air(&e).as_deref(), Some("a1"));
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

    // Timers and loops (T34). Time is a plain number of milliseconds.

    fn at(ms: i64) -> Timestamp {
        Timestamp(1_000_000 + ms)
    }

    fn looping(ms: u32, slides: &[&str]) -> LoadedItem {
        LoadedItem {
            auto_advance_ms: Some(ms),
            ..item("Announcements", slides.iter().map(|s| text(s)).collect())
        }
    }

    fn countdown_item(target: Timestamp) -> LoadedItem {
        item(
            "Countdown",
            vec![LoadedSlide {
                content: Content::Countdown {
                    target,
                    label: "Starts".into(),
                },
                steps: 0,
            }],
        )
    }

    /// An engine with the loop live and ticked once at `at(0)`.
    fn loop_engine(ms: u32) -> LiveEngine {
        let mut e = LiveEngine::new();
        e.load(vec![
            looping(ms, &["l1", "l2", "l3"]),
            item("Song", vec![text("s1")]),
        ]);
        e.tick(at(0));
        e.apply(Command::Next);
        e.tick(at(0));
        e
    }

    #[test]
    fn loop_advances_on_its_interval_and_wraps() {
        let mut e = loop_engine(5000);
        assert_eq!(on_air(&e).as_deref(), Some("l1"));
        assert!(e.tick(at(4900)).is_empty());
        assert_eq!(e.tick(at(5000)), [Event::ProgramChanged]);
        assert_eq!(on_air(&e).as_deref(), Some("l2"));
        e.tick(at(10_000));
        e.tick(at(15_000));
        assert_eq!(on_air(&e).as_deref(), Some("l1"), "wrapped");
        assert_eq!(e.live().map(|c| c.step), Some(0));
    }

    #[test]
    fn loop_keeps_its_cadence_but_does_not_catch_up_after_a_sleep() {
        let mut e = loop_engine(5000);
        // A tick 200 ms late does not push the next one back.
        e.tick(at(5200));
        assert!(e.tick(at(9900)).is_empty());
        assert!(!e.tick(at(10_000)).is_empty());
        assert_eq!(on_air(&e).as_deref(), Some("l3"));
        // The machine sleeps for ten minutes: one step, then a fresh interval.
        e.tick(at(600_000));
        assert_eq!(on_air(&e).as_deref(), Some("l1"));
        assert!(e.tick(at(604_900)).is_empty());
        assert!(!e.tick(at(605_000)).is_empty());
    }

    #[test]
    fn loop_interval_runs_from_the_first_tick_after_going_live() {
        let mut e = LiveEngine::new();
        e.load(vec![looping(1000, &["a", "b"])]);
        e.tick(at(0));
        e.apply(Command::Next);
        // The first tick only starts the clock.
        assert!(e.tick(at(700)).is_empty());
        assert!(e.tick(at(1600)).is_empty());
        assert!(!e.tick(at(1700)).is_empty());
    }

    #[test]
    fn clock_going_backwards_restarts_the_interval() {
        let mut e = loop_engine(5000);
        assert!(e.tick(at(-10_000)).is_empty());
        assert!(e.tick(at(-6000)).is_empty());
        assert!(!e.tick(at(-5000)).is_empty());
    }

    #[test]
    fn single_slide_and_plain_items_never_advance() {
        let mut e = LiveEngine::new();
        e.load(vec![
            looping(1000, &["only"]),
            item("Song", vec![text("s1"), text("s2")]),
        ]);
        e.apply(Command::Next);
        e.tick(at(0));
        assert!(e.tick(at(60_000)).is_empty());
        e.apply(Command::Next);
        e.tick(at(61_000));
        assert!(e.tick(at(120_000)).is_empty());
        assert_eq!(on_air(&e).as_deref(), Some("s1"));
    }

    #[test]
    fn tick_with_nothing_live_does_nothing() {
        let (mut e, _) = service();
        assert!(e.tick(at(0)).is_empty());
        assert!(e.tick(at(99_999)).is_empty());
        assert!(e.is_idle());
    }

    #[test]
    fn manual_moves_restart_the_loop_interval() {
        let mut e = loop_engine(5000);
        e.tick(at(4000));
        e.apply(Command::Next);
        assert_eq!(on_air(&e).as_deref(), Some("l2"));
        // The interval starts at the first tick after the move.
        assert!(e.tick(at(4100)).is_empty());
        assert!(e.tick(at(9000)).is_empty());
        assert!(!e.tick(at(9100)).is_empty());
        assert_eq!(on_air(&e).as_deref(), Some("l3"));
        // Previous restarts it too.
        e.tick(at(10_000));
        e.apply(Command::Previous);
        assert_eq!(on_air(&e).as_deref(), Some("l2"));
        e.tick(at(10_100));
        assert!(e.tick(at(15_000)).is_empty());
        assert!(!e.tick(at(15_100)).is_empty());
    }

    #[test]
    fn next_on_the_last_loop_slide_leaves_the_loop() {
        let mut e = loop_engine(5000);
        e.apply(Command::Next);
        e.apply(Command::Next);
        e.apply(Command::Next);
        assert_eq!(on_air(&e).as_deref(), Some("s1"));
        assert!(e.tick(at(1_000_000)).is_empty());
    }

    #[test]
    fn black_does_not_stop_the_loop() {
        let mut e = loop_engine(5000);
        e.apply(Command::ToggleBlack);
        e.tick(at(5000));
        assert_eq!(on_air(&e).as_deref(), Some("l2"));
        assert_eq!(e.program().overlay, Overlay::Black);
        e.apply(Command::ToggleBlack);
        assert_eq!(on_air(&e).as_deref(), Some("l2"));
    }

    #[test]
    fn clear_text_and_clear_all_stop_the_loop_until_something_goes_live() {
        for clear in [Command::ClearText, Command::ClearAll] {
            let mut e = loop_engine(5000);
            e.apply(clear);
            assert!(e.tick(at(5000)).is_empty());
            assert!(e.tick(at(50_000)).is_empty());
            assert_eq!(on_air(&e), None);
            let live = e.live().unwrap();
            e.apply(Command::Show(live));
            e.tick(at(51_000));
            assert!(e.tick(at(55_000)).is_empty());
            assert!(!e.tick(at(56_000)).is_empty());
            assert_eq!(on_air(&e).as_deref(), Some("l2"));
        }
    }

    #[test]
    fn clear_background_leaves_the_loop_running() {
        let mut e = loop_engine(5000);
        e.apply(Command::ClearBackground);
        e.tick(at(5000));
        assert_eq!(on_air(&e).as_deref(), Some("l2"));
        assert!(
            e.program().background.is_some(),
            "the next slide restores it"
        );
    }

    #[test]
    fn loop_survives_a_reload_that_deletes_it() {
        let mut e = loop_engine(5000);
        e.load(vec![item("Song", vec![text("s1")])]);
        e.tick(at(5000));
        assert_eq!(on_air(&e).as_deref(), Some("l2"));
    }

    #[test]
    fn countdown_reports_each_second_then_holds_zero() {
        let mut e = LiveEngine::new();
        e.load(vec![countdown_item(at(3000))]);
        e.tick(at(0));
        e.apply(Command::Next);
        // Whole seconds round up, as on screen: 2.001 s still shows 3.
        assert!(e.tick(at(100)).is_empty());
        assert!(e.tick(at(999)).is_empty());
        assert_eq!(e.tick(at(1000)), [Event::ProgramChanged]);
        assert!(e.tick(at(1100)).is_empty());
        assert_eq!(e.tick(at(2000)), [Event::ProgramChanged]);
        assert_eq!(e.tick(at(3000)), [Event::ProgramChanged], "reaches zero");
        assert!(e.tick(at(3100)).is_empty());
        assert!(e.tick(at(600_000)).is_empty());
        assert!(matches!(
            e.program().content,
            Some(Content::Countdown { .. })
        ));
        assert!(!e.is_idle());
    }

    #[test]
    fn countdown_already_past_reports_nothing() {
        let mut e = LiveEngine::new();
        e.load(vec![countdown_item(at(-5000))]);
        e.tick(at(0));
        e.apply(Command::Next);
        assert!(e.tick(at(100)).is_empty());
        assert!(e.tick(at(10_000)).is_empty());
    }

    #[test]
    fn countdown_target_is_an_absolute_instant_whatever_the_local_time() {
        // A countdown to a time of day arrives as an absolute Timestamp,
        // so the engine's output is the same in every time zone.
        let mut e = LiveEngine::new();
        e.load(vec![countdown_item(Timestamp(1_800_000_090_000))]);
        e.tick(Timestamp(1_800_000_000_000));
        e.apply(Command::Next);
        assert!(e.tick(Timestamp(1_800_000_000_500)).is_empty());
        assert_eq!(
            e.tick(Timestamp(1_800_000_001_000)),
            [Event::ProgramChanged]
        );
    }

    #[test]
    fn countdown_is_quiet_under_black_and_after_clear() {
        let mut e = LiveEngine::new();
        e.load(vec![countdown_item(at(10_000))]);
        e.tick(at(0));
        e.apply(Command::Next);
        e.apply(Command::ToggleBlack);
        assert!(e.tick(at(1000)).is_empty());
        e.apply(Command::ToggleBlack);
        assert_eq!(e.tick(at(2000)), [Event::ProgramChanged]);
        e.apply(Command::ClearText);
        assert!(e.tick(at(3000)).is_empty());
    }

    #[test]
    fn stage_next_countdown_updates_too() {
        let mut e = LiveEngine::new();
        e.load(vec![
            item("Welcome", vec![text("a1")]),
            countdown_item(at(10_000)),
        ]);
        e.tick(at(0));
        e.apply(Command::Next);
        assert!(e.tick(at(500)).is_empty());
        assert_eq!(e.tick(at(1000)), [Event::ProgramChanged]);
    }

    #[test]
    fn a_running_loop_or_countdown_is_not_idle() {
        let mut e = loop_engine(5000);
        assert!(!e.is_idle());
        e.tick(at(5000));
        assert!(!e.is_idle());
        e.apply(Command::ClearAll);
        assert!(!e.is_idle(), "the service is still under way");

        let mut e = LiveEngine::new();
        e.load(vec![countdown_item(at(2000))]);
        assert!(e.is_idle());
        e.tick(at(0));
        e.apply(Command::Next);
        assert!(!e.is_idle());
        e.tick(at(5000));
        assert!(!e.is_idle(), "a finished countdown is still on air");
    }
}
