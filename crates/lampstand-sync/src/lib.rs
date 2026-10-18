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

pub mod team;

use std::path::PathBuf;

use lampstand_core::id::BlobHash;
use uuid::Uuid;

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

/// A core type stored as a synced document. T16 adds the autosurgeon
/// `Hydrate + Reconcile` bounds and implements this for each core type.
pub trait Doc: Sized {
    const KIND: DocKind;
    fn doc_id(&self) -> Uuid;
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

/// All documents and blobs on this device.
pub struct Repo {
    _private: (),
}

impl Repo {
    /// Opens (or creates) the store under `dir`: `dir/docs/<id>` holds each
    /// document's saved chunks, `dir/blobs/<hash>` each blob.
    pub fn open(dir: PathBuf, device: team::DeviceKeypair) -> Result<Self, SyncError> {
        let _ = (dir, device);
        todo!("T16")
    }

    pub fn get<T: Doc>(&self, id: Uuid) -> Result<Option<T>, SyncError> {
        let _ = id;
        todo!("T16")
    }

    /// Writes `doc`, diffing against the stored document so that only the
    /// fields that changed become CRDT operations.
    pub fn put<T: Doc>(&self, doc: &T) -> Result<(), SyncError> {
        let _ = doc;
        todo!("T16")
    }

    /// Deletes by tombstone in the team document's index.
    pub fn delete(&self, kind: DocKind, id: Uuid) -> Result<(), SyncError> {
        let _ = (kind, id);
        todo!("T16")
    }

    pub fn list(&self, kind: DocKind) -> Vec<Uuid> {
        let _ = kind;
        todo!("T16")
    }

    /// Every document change, local or remote.
    pub fn subscribe(&self) -> std::sync::mpsc::Receiver<DocChanged> {
        todo!("T16")
    }

    pub fn blobs(&self) -> &BlobStore {
        todo!("T16")
    }
}

/// Content-addressed files. Writes are atomic (temp file then rename) and
/// verified against their hash.
pub struct BlobStore {
    _private: (),
}

impl BlobStore {
    pub fn put(&self, bytes: &[u8]) -> Result<BlobHash, SyncError> {
        let _ = bytes;
        todo!("T16")
    }

    pub fn path(&self, hash: &BlobHash) -> Option<PathBuf> {
        let _ = hash;
        todo!("T16")
    }

    /// The hashes in `wanted` this device does not have yet.
    pub fn missing(&self, wanted: &[BlobHash]) -> Vec<BlobHash> {
        let _ = wanted;
        todo!("T16")
    }
}
