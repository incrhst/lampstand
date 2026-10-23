//! Local-first sync: every device holds the full library and services and
//! merges changes over the church network, a relay or (Phase 2) a shared
//! folder.
//!
//! PRD: CD-1 to CD-3, CD-6.
//!
//! - One Automerge document per entity (song, service, theme, deck,
//!   translation, usage log) plus one team document. Documents map to the
//!   core types with autosurgeon; the document ID is the entity's `Id`.
//! - Blobs (deck files, media, Bible text) are content-addressed files,
//!   fetched by hash from whichever peer has them. Never inside documents.
//! - One wire protocol for every link: a WebSocket carrying Automerge sync
//!   messages and blob requests. LAN peers find each other by mDNS
//!   (`_lampstand._tcp`); the relay is the same protocol on a server.
//! - Connections authenticate with device keys; see [`team`].
//!
//! The repo wraps `automerge` directly rather than using samod; see
//! docs/decisions/0002-sync-repo.md.

pub mod driver;
#[cfg(feature = "net")]
pub mod net;
pub mod presenter;
mod repo;
pub mod team;

use std::fmt;

use lampstand_core::deck::Deck;
use lampstand_core::scripture::Translation;
use lampstand_core::service::Service;
use lampstand_core::song::Song;
use lampstand_core::theme::Theme;
use uuid::Uuid;

pub use repo::{actor_device, BlobStore, Repo, SyncMessage};

/// The kinds of synced document. Each has its own write permission
/// ([`team::Role::can_write`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DocKind {
    Team,
    Song,
    Service,
    Theme,
    Deck,
    Translation,
    /// One per device: the CCLI usage log of what that device showed.
    UsageLog,
}

impl DocKind {
    pub const ALL: [DocKind; 7] = [
        DocKind::Team,
        DocKind::Song,
        DocKind::Service,
        DocKind::Theme,
        DocKind::Deck,
        DocKind::Translation,
        DocKind::UsageLog,
    ];

    /// The name used in the team document's index and on the wire.
    pub fn as_str(self) -> &'static str {
        match self {
            DocKind::Team => "team",
            DocKind::Song => "song",
            DocKind::Service => "service",
            DocKind::Theme => "theme",
            DocKind::Deck => "deck",
            DocKind::Translation => "translation",
            DocKind::UsageLog => "usage-log",
        }
    }

    pub fn parse(s: &str) -> Option<DocKind> {
        DocKind::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

/// A core type stored as a synced document: the type is reconciled onto
/// the document's root map, so a `put` writes only the fields that changed.
pub trait Doc: Sized + autosurgeon::Hydrate + autosurgeon::Reconcile {
    const KIND: DocKind;
    fn doc_id(&self) -> Uuid;
}

impl Doc for Song {
    const KIND: DocKind = DocKind::Song;
    fn doc_id(&self) -> Uuid {
        self.id.uuid()
    }
}

impl Doc for Service {
    const KIND: DocKind = DocKind::Service;
    fn doc_id(&self) -> Uuid {
        self.id.uuid()
    }
}

impl Doc for Theme {
    const KIND: DocKind = DocKind::Theme;
    fn doc_id(&self) -> Uuid {
        self.id.uuid()
    }
}

impl Doc for Deck {
    const KIND: DocKind = DocKind::Deck;
    fn doc_id(&self) -> Uuid {
        self.id.uuid()
    }
}

impl Doc for Translation {
    const KIND: DocKind = DocKind::Translation;
    fn doc_id(&self) -> Uuid {
        self.id.uuid()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocChanged {
    pub kind: DocKind,
    pub id: Uuid,
    /// False when the change came from this device.
    pub remote: bool,
}

#[derive(Debug)]
pub enum SyncError {
    Io(std::io::Error),
    Corrupt(String),
    /// The local role may not write this kind of document.
    Forbidden(DocKind),
}

impl fmt::Display for SyncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SyncError::Io(e) => write!(f, "sync store I/O: {e}"),
            SyncError::Corrupt(why) => write!(f, "corrupt sync data: {why}"),
            SyncError::Forbidden(kind) => write!(f, "this device may not write {}", kind.as_str()),
        }
    }
}

impl std::error::Error for SyncError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SyncError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for SyncError {
    fn from(e: std::io::Error) -> Self {
        SyncError::Io(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_names_round_trip() {
        for kind in DocKind::ALL {
            assert_eq!(DocKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(DocKind::parse("hymn"), None);
    }
}
