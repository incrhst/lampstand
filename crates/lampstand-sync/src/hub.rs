use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use automerge::sync;
use automerge::{AutoCommit, AutomergeError};
use tokio::sync::broadcast;

use crate::{DocId, Store, SyncError};

/// A [`Store`] shared by the app and every sync session.
///
/// Edits made through the hub, and changes that arrive from one peer, are
/// announced to every session so they reach the other peers straight away.
pub struct Hub {
    store: Mutex<Store>,
    changed: broadcast::Sender<DocId>,
    peer_id: String,
}

impl Hub {
    pub fn new(store: Store) -> Arc<Self> {
        let (changed, _) = broadcast::channel(256);
        Arc::new(Self {
            peer_id: store.actor_hex(),
            store: Mutex::new(store),
            changed,
        })
    }

    pub fn open(dir: impl Into<PathBuf>) -> Result<Arc<Self>, SyncError> {
        Ok(Self::new(Store::open(dir)?))
    }

    /// This device's id on the sync network: its Automerge actor id.
    pub fn peer_id(&self) -> &str {
        &self.peer_id
    }

    /// Locks the store. Prefer [`Hub::edit`] and [`Hub::create`] for writes
    /// so peers hear about them; call [`Hub::notify`] after writing directly.
    pub fn store(&self) -> MutexGuard<'_, Store> {
        // A panic while holding the lock leaves the in-memory documents
        // intact (Automerge rolls back open transactions), so carry on.
        self.store.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// See [`Store::create`].
    pub fn create(
        &self,
        id: &DocId,
        init: impl FnOnce(&mut AutoCommit) -> Result<(), AutomergeError>,
    ) -> Result<(), SyncError> {
        self.store().create(id, init)?;
        self.notify(id);
        Ok(())
    }

    /// See [`Store::edit`].
    pub fn edit<R>(
        &self,
        id: &DocId,
        message: &str,
        f: impl FnOnce(&mut AutoCommit) -> Result<R, AutomergeError>,
    ) -> Result<R, SyncError> {
        let result = self.store().edit(id, message, f)?;
        self.notify(id);
        Ok(result)
    }

    /// Tells every sync session that `id` changed.
    pub fn notify(&self, id: &DocId) {
        // An error only means no session is listening.
        let _ = self.changed.send(id.clone());
    }

    /// A feed of ids of documents that changed.
    pub fn subscribe(&self) -> broadcast::Receiver<DocId> {
        self.changed.subscribe()
    }

    pub(crate) fn generate_sync_message(
        &self,
        id: &DocId,
        state: &mut sync::State,
    ) -> Option<sync::Message> {
        self.store().generate_sync_message(id, state)
    }

    pub(crate) fn receive_sync_message(
        &self,
        id: &DocId,
        state: &mut sync::State,
        message: sync::Message,
    ) -> Result<(), SyncError> {
        if self.store().receive_sync_message(id, state, message)? {
            self.notify(id);
        }
        Ok(())
    }
}
