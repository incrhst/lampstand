//! A service order: the songs, scripture, decks and media for one service.
//!
//! PRD: PL-1, PS-8.
//!
//! Items live in a map keyed by ID, each with a fractional `Position`,
//! rather than in a list. Two planners reordering the same service offline
//! then merge cleanly: a CRDT list has no "move", so a move is a delete
//! plus an insert and concurrent moves duplicate the item.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::deck::DeckId;
use crate::id::{BlobHash, Id, Timestamp};
use crate::scripture::{Passage, TranslationId};
use crate::song::SongId;
use crate::theme::ThemeId;

pub type ServiceId = Id<Service>;
pub type ItemId = Id<ServiceItem>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "automerge",
    derive(autosurgeon::Hydrate, autosurgeon::Reconcile)
)]
pub struct Service {
    pub id: ServiceId,
    pub title: String,
    pub starts_at: Option<Timestamp>,
    /// Default theme for text items; each output may override it.
    pub theme: Option<ThemeId>,
    /// Keyed by the item ID as a string in the synced document.
    #[cfg_attr(
        feature = "automerge",
        autosurgeon(with = "autosurgeon::map_with_parseable_keys")
    )]
    pub items: BTreeMap<ItemId, ServiceItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "automerge",
    derive(autosurgeon::Hydrate, autosurgeon::Reconcile)
)]
pub struct ServiceItem {
    pub position: Position,
    pub kind: ItemKind,
    /// Planner's notes for the operator; never projected.
    pub notes: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "automerge",
    derive(autosurgeon::Hydrate, autosurgeon::Reconcile)
)]
pub enum ItemKind {
    /// A divider in the order ("Worship", "Sermon"). Has no slides.
    Header {
        title: String,
    },
    Song {
        song: SongId,
        /// This service's arrangement, if it differs from the song's.
        arrangement: Option<Vec<String>>,
    },
    Scripture {
        passage: Passage,
        translation: TranslationId,
    },
    Deck {
        deck: DeckId,
    },
    Media {
        blob: BlobHash,
        name: String,
        looped: bool,
    },
    Countdown {
        target: CountdownTarget,
        label: String,
    },
    /// Images shown in a loop, for example before the service.
    Announcements {
        slides: Vec<BlobHash>,
        seconds_each: u16,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "automerge",
    derive(autosurgeon::Hydrate, autosurgeon::Reconcile)
)]
pub enum CountdownTarget {
    /// Counts down to a wall-clock time (the service start).
    At(Timestamp),
    /// Counts down this many seconds from going live.
    Seconds(u32),
}

/// Fractional index: items sort by position string, then by ID when two
/// devices picked the same position concurrently. Positions only contain
/// the characters `0-9A-Za-z`, so they sort the same as bytes everywhere.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(
    feature = "automerge",
    derive(autosurgeon::Hydrate, autosurgeon::Reconcile)
)]
pub struct Position(String);

impl Position {
    /// A position strictly between two others (either may be open). Keeps
    /// strings short: repeated inserts at the same spot grow them by about
    /// one character per 60 inserts.
    pub fn between(before: Option<&Position>, after: Option<&Position>) -> Position {
        let _ = (before, after);
        todo!("T02")
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Service {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            id: ServiceId::generate(),
            title: title.into(),
            starts_at: None,
            theme: None,
            items: BTreeMap::new(),
        }
    }

    /// The items in running order.
    pub fn ordered(&self) -> Vec<(ItemId, &ServiceItem)> {
        let mut items: Vec<_> = self.items.iter().map(|(id, item)| (*id, item)).collect();
        items.sort_by(|(a_id, a), (b_id, b)| a.position.cmp(&b.position).then(a_id.cmp(b_id)));
        items
    }

    /// Inserts a new item so that it lands at `index` in running order
    /// (clamped to the end).
    pub fn insert(&mut self, index: usize, kind: ItemKind) -> ItemId {
        let _ = (index, kind);
        todo!("T02")
    }

    /// Moves an item to `index` in running order, changing only its own
    /// position. Returns false if there is no such item.
    pub fn move_item(&mut self, id: ItemId, index: usize) -> bool {
        let _ = (id, index);
        todo!("T02")
    }

    pub fn remove(&mut self, id: ItemId) -> Option<ServiceItem> {
        self.items.remove(&id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(title: &str) -> ItemKind {
        ItemKind::Header {
            title: title.into(),
        }
    }

    fn titles(service: &Service) -> Vec<String> {
        service
            .ordered()
            .into_iter()
            .map(|(_, item)| match &item.kind {
                ItemKind::Header { title } => title.clone(),
                _ => unreachable!(),
            })
            .collect()
    }

    #[test]
    #[ignore = "T02"]
    fn positions_sort_between_neighbours() {
        let a = Position::between(None, None);
        let b = Position::between(Some(&a), None);
        let mid = Position::between(Some(&a), Some(&b));
        let first = Position::between(None, Some(&a));
        assert!(first < a && a < mid && mid < b);
    }

    #[test]
    #[ignore = "T02"]
    fn many_inserts_at_one_spot_stay_ordered_and_short() {
        let low = Position::between(None, None);
        let mut high = Position::between(Some(&low), None);
        for _ in 0..1000 {
            let p = Position::between(Some(&low), Some(&high));
            assert!(low < p && p < high);
            high = p;
        }
        assert!(high.as_str().len() < 40, "{}", high.as_str());
        assert!(high.as_str().chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    #[ignore = "T02"]
    fn insert_and_move_keep_running_order() {
        let mut s = Service::new("Sunday");
        s.insert(0, header("B"));
        s.insert(0, header("A"));
        s.insert(9, header("D"));
        let c = s.insert(2, header("C"));
        assert_eq!(titles(&s), ["A", "B", "C", "D"]);
        assert!(s.move_item(c, 0));
        assert_eq!(titles(&s), ["C", "A", "B", "D"]);
        assert!(s.move_item(c, 3));
        assert_eq!(titles(&s), ["A", "B", "D", "C"]);
    }

    #[test]
    fn equal_positions_break_ties_by_id() {
        let mut s = Service::new("Sunday");
        let p = Position("U".into());
        for title in ["x", "y"] {
            let item = ServiceItem {
                position: p.clone(),
                kind: header(title),
                notes: String::new(),
            };
            s.items.insert(ItemId::generate(), item);
        }
        let ids: Vec<ItemId> = s.ordered().into_iter().map(|(id, _)| id).collect();
        assert!(ids[0] < ids[1]);
    }
}
