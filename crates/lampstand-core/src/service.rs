//! Services: the ordered list of songs, scripture and decks run on the day.
//!
//! PRD: PL-1, PS-8, BI-1.

use autosurgeon::{Hydrate, Reconcile};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, Reconcile, Hydrate)]
pub struct Service {
    pub title: String,
    /// ISO 8601 date, such as "2026-10-25".
    pub date: Option<String>,
    pub items: Vec<ServiceItem>,
}

/// One entry in the service order. The id identifies the item when edits
/// from several devices merge, so moving an item is not read as an edit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Reconcile, Hydrate)]
pub struct ServiceItem {
    #[key]
    pub id: String,
    pub kind: ItemKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Reconcile, Hydrate)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ItemKind {
    /// A song from the library, by its library id.
    Song { song_id: String },
    /// A Bible reference as typed, such as "Jn 3:16-18".
    Scripture { reference: String },
    /// An imported PowerPoint or Google Slides deck, by its id.
    Deck { deck_id: String },
}
