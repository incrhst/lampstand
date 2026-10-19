//! The readiness check (CD-10): is everything for this service on this
//! machine? One green or red status, with the reasons.
//!
//! Also drives pre-service caching (PS-6): `prepare` fetches missing blobs
//! from peers and refreshes linked decks before reporting.

use lampstand_core::service::{ItemId, ServiceId};

use crate::Library;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Readiness {
    pub items: Vec<ItemReadiness>,
}

impl Readiness {
    pub fn is_ready(&self) -> bool {
        self.items.iter().all(|i| i.problems.is_empty())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemReadiness {
    pub item: ItemId,
    pub problems: Vec<Problem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    SongMissing,
    /// A deck, image or video file is not on this machine yet.
    BlobMissing {
        name: String,
    },
    TranslationMissing {
        abbreviation: String,
    },
    /// A linked deck has a newer version nobody has accepted.
    DeckUpdatePending,
    /// The fidelity report found issues (shown, not blocking).
    DeckFidelity {
        issues: usize,
    },
    /// Text below the theme's minimum size on some output (TS-7).
    TextTooSmall {
        slides: Vec<usize>,
    },
}

/// Checks without touching the network.
pub fn check(library: &Library, service: ServiceId) -> Readiness {
    let _ = (library, service);
    todo!("T36")
}
