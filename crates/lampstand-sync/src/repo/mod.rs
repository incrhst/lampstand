//! The repo: documents in memory and on disk, the team document's index,
//! per-peer sync state and the blob store.

mod blobs;
mod fsutil;
mod store;

use std::collections::hash_map::Entry as MapEntry;
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, MutexGuard};

use automerge::sync::{self, SyncDoc};
use automerge::transaction::{CommitOptions, Transactable};
use automerge::{
    ActorId, Automerge, AutomergeError, ChangeHash, ObjId, ObjType, ReadDoc, Value, ROOT,
};
use uuid::Uuid;

pub use blobs::BlobStore;

use crate::team::{DeviceKey, DeviceKeypair};
use crate::{Doc, DocChanged, DocKind, SyncError};
use store::DocStore;

/// The team document's ID until T18 gives each team its own.
const TEAM_DOC: Uuid = Uuid::nil();

/// Incremental chunks a document collects on disk before it is rewritten
/// as one snapshot.
const COMPACT_AFTER: usize = 32;

/// Bytes of randomness after the device key in an actor ID.
const ACTOR_TAIL: usize = 8;

/// One Automerge sync message for one document. On the wire (T17) the kind
/// travels with it, so a peer can create a document it has not seen with
/// the right kind and check the sender may write that kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncMessage {
    pub kind: DocKind,
    pub id: Uuid,
    /// An encoded `automerge::sync::Message`.
    pub data: Vec<u8>,
}

/// The device that made a change, from the change's actor ID: the first 32
/// bytes are the device's public key (see [`Repo::open`]).
pub fn actor_device(actor: &ActorId) -> Option<DeviceKey> {
    let bytes = actor.to_bytes();
    (bytes.len() == 32 + ACTOR_TAIL).then(|| DeviceKey(bytes[..32].try_into().expect("32 bytes")))
}

/// All documents and blobs on this device.
pub struct Repo {
    device: DeviceKey,
    actor: ActorId,
    store: DocStore,
    blobs: BlobStore,
    state: Mutex<State>,
    subscribers: Mutex<Vec<Sender<DocChanged>>>,
}

#[derive(Default)]
struct State {
    docs: HashMap<Uuid, Loaded>,
    peers: HashMap<(DeviceKey, Uuid), sync::State>,
}

/// A document in memory, and how much of it is on disk.
struct Loaded {
    kind: DocKind,
    doc: Automerge,
    saved: Vec<ChangeHash>,
    chunks: usize,
}

impl Repo {
    /// Opens (or creates) the store under `dir`: `dir/docs/<id>` holds each
    /// document's saved chunks, `dir/blobs/<hash>` each blob.
    ///
    /// Changes made here carry an actor ID of the device's public key
    /// followed by 8 random bytes chosen at each open. Peers read the key
    /// to check the author's role; the random tail means a device that
    /// lost its store can never reuse an (actor, sequence number) pair.
    pub fn open(dir: PathBuf, device: DeviceKeypair) -> Result<Self, SyncError> {
        let device = device.public();
        let mut actor = device.0.to_vec();
        actor.extend(fsutil::random_bytes::<ACTOR_TAIL>());
        let repo = Self {
            device,
            actor: ActorId::from(actor),
            store: DocStore::open(dir.join("docs"))?,
            blobs: BlobStore::open(dir.join("blobs"))?,
            state: Mutex::new(State::default()),
            subscribers: Mutex::new(Vec::new()),
        };
        {
            let mut st = repo.lock();
            if repo.load(&mut st.docs, TEAM_DOC, None)?.is_none() {
                let doc = team_skeleton().with_actor(repo.actor.clone());
                let mut team = Loaded {
                    kind: DocKind::Team,
                    doc,
                    saved: Vec::new(),
                    chunks: 0,
                };
                repo.persist(TEAM_DOC, &mut team)?;
                st.docs.insert(TEAM_DOC, team);
            }
        }
        Ok(repo)
    }

    /// This device's public key.
    pub fn device(&self) -> DeviceKey {
        self.device
    }

    pub fn get<T: Doc>(&self, id: Uuid) -> Result<Option<T>, SyncError> {
        let mut st = self.lock();
        if self.tombstoned(&mut st.docs, id)? {
            return Ok(None);
        }
        let Some(loaded) = self.load(&mut st.docs, id, None)? else {
            return Ok(None);
        };
        if loaded.kind != T::KIND || loaded.doc.get_heads().is_empty() {
            return Ok(None);
        }
        autosurgeon::hydrate(&loaded.doc)
            .map(Some)
            .map_err(|e| SyncError::Corrupt(format!("{} {id}: {e}", T::KIND.as_str())))
    }

    /// Writes `doc`, diffing against the stored document so that only the
    /// fields that changed become CRDT operations.
    pub fn put<T: Doc>(&self, doc: &T) -> Result<(), SyncError> {
        let id = doc.doc_id();
        let mut st = self.lock();
        let loaded = self
            .load(&mut st.docs, id, Some(T::KIND))?
            .expect("created when missing");
        check_kind(id, loaded.kind, T::KIND)?;
        let before = loaded.doc.get_heads();
        loaded
            .doc
            .transact::<_, _, autosurgeon::ReconcileError>(|tx| autosurgeon::reconcile(tx, doc))
            .map_err(|f| SyncError::Corrupt(format!("{} {id}: {}", T::KIND.as_str(), f.error)))?;
        let changed = loaded.doc.get_heads() != before;
        if changed {
            self.persist(id, loaded)?;
        }
        let indexed = self.set_index(&mut st.docs, id, T::KIND, false)?;
        drop(st);
        if changed || indexed {
            self.notify(T::KIND, id, false);
        }
        Ok(())
    }

    /// Deletes by tombstone in the team document's index. The document
    /// itself is kept, so the deletion syncs like any other change and a
    /// later `put` of the same ID brings it back.
    pub fn delete(&self, kind: DocKind, id: Uuid) -> Result<(), SyncError> {
        let mut st = self.lock();
        let changed = self.set_index(&mut st.docs, id, kind, true)?;
        drop(st);
        if changed {
            self.notify(kind, id, false);
        }
        Ok(())
    }

    /// The live (not deleted) documents of `kind` in the team index, in ID
    /// order (which is creation order, as IDs are UUIDv7).
    pub fn list(&self, kind: DocKind) -> Vec<Uuid> {
        let mut st = self.lock();
        let Ok(Some(team)) = self.load(&mut st.docs, TEAM_DOC, None) else {
            return Vec::new();
        };
        let (Some(index), Some(deleted)) =
            (map_at(&team.doc, "index"), map_at(&team.doc, "deleted"))
        else {
            return Vec::new();
        };
        let doc = &team.doc;
        let mut ids: Vec<Uuid> = doc
            .keys(&index)
            .filter(|key| {
                matches!(doc.get(&index, key.as_str()), Ok(Some((v, _))) if v.as_str() == Some(kind.as_str()))
            })
            .filter(|key| matches!(doc.get(&deleted, key.as_str()), Ok(None)))
            .filter_map(|key| key.parse().ok())
            .collect();
        ids.sort();
        ids
    }

    /// Whether `id` is a document of `kind` that was deleted, as opposed to
    /// one that never existed (both make [`Repo::get`] return `None`).
    pub fn is_deleted(&self, kind: DocKind, id: Uuid) -> Result<bool, SyncError> {
        let mut st = self.lock();
        if !self.tombstoned(&mut st.docs, id)? {
            return Ok(false);
        }
        let Some(team) = self.load(&mut st.docs, TEAM_DOC, None)? else {
            return Ok(false);
        };
        let Some(index) = map_at(&team.doc, "index") else {
            return Ok(false);
        };
        Ok(matches!(
            team.doc.get(&index, id.to_string()),
            Ok(Some((v, _))) if v.as_str() == Some(kind.as_str())
        ))
    }

    /// Every document change, local or remote.
    pub fn subscribe(&self) -> std::sync::mpsc::Receiver<DocChanged> {
        let (tx, rx) = mpsc::channel();
        self.subscribers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(tx);
        rx
    }

    pub fn blobs(&self) -> &BlobStore {
        &self.blobs
    }

    /// Every document this device holds, the team document and deleted
    /// ones included: what to offer a peer when a connection opens.
    pub fn documents(&self) -> Vec<(DocKind, Uuid)> {
        let st = self.lock();
        let mut all: BTreeSet<(DocKind, Uuid)> = self
            .store
            .ids()
            .unwrap_or_default()
            .into_iter()
            .map(|(id, kind)| (kind, id))
            .collect();
        all.extend(
            st.docs
                .iter()
                .filter(|(_, d)| !d.doc.get_heads().is_empty())
                .map(|(id, d)| (d.kind, *id)),
        );
        all.into_iter().collect()
    }

    /// The next sync message for `peer` about document `id`, or `None` when
    /// there is nothing to say until the peer replies. Unknown IDs give
    /// `None`.
    pub fn generate_sync_message(&self, peer: DeviceKey, id: Uuid) -> Option<SyncMessage> {
        let mut st = self.lock();
        let State { docs, peers } = &mut *st;
        let loaded = self.load(docs, id, None).ok()??;
        let state = peers.entry((peer, id)).or_default();
        loaded
            .doc
            .generate_sync_message(state)
            .map(|m| SyncMessage {
                kind: loaded.kind,
                id,
                data: m.encode(),
            })
    }

    /// Applies a sync message from `peer`, creating the document if this
    /// device has not seen it.
    pub fn receive_sync_message(
        &self,
        peer: DeviceKey,
        message: SyncMessage,
    ) -> Result<(), SyncError> {
        let SyncMessage { kind, id, data } = message;
        let decoded = sync::Message::decode(&data)
            .map_err(|e| SyncError::Corrupt(format!("sync message for {id}: {e}")))?;
        let mut st = self.lock();
        let State { docs, peers } = &mut *st;
        let loaded = self
            .load(docs, id, Some(kind))?
            .expect("created when missing");
        check_kind(id, loaded.kind, kind)?;
        // T18: drop changes whose author's role may not write `kind`.
        let before = loaded.doc.get_heads();
        loaded
            .doc
            .receive_sync_message(peers.entry((peer, id)).or_default(), decoded)
            .map_err(|e| SyncError::Corrupt(format!("sync message for {id}: {e}")))?;
        let changed = loaded.doc.get_heads() != before;
        if changed {
            self.persist(id, loaded)?;
        }
        drop(st);
        if changed {
            self.notify(kind, id, true);
        }
        Ok(())
    }

    /// Drops the sync state kept for `peer`, when its connection closes.
    /// The next connection starts the sync handshake afresh.
    pub fn forget_peer(&self, peer: DeviceKey) {
        self.lock().peers.retain(|(p, _), _| *p != peer);
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The document `id`, loaded from disk if needed. If it is nowhere and
    /// `create` names a kind, an empty document of that kind.
    fn load<'a>(
        &self,
        docs: &'a mut HashMap<Uuid, Loaded>,
        id: Uuid,
        create: Option<DocKind>,
    ) -> Result<Option<&'a mut Loaded>, SyncError> {
        match docs.entry(id) {
            MapEntry::Occupied(e) => Ok(Some(e.into_mut())),
            MapEntry::Vacant(e) => {
                let loaded = match self.store.load(id)? {
                    Some(stored) => {
                        let mut doc = stored.doc;
                        doc.set_actor(self.actor.clone());
                        Loaded {
                            kind: stored.kind,
                            saved: doc.get_heads(),
                            doc,
                            chunks: stored.chunks,
                        }
                    }
                    None => match create {
                        Some(kind) => Loaded {
                            kind,
                            doc: Automerge::new().with_actor(self.actor.clone()),
                            saved: Vec::new(),
                            chunks: 0,
                        },
                        None => return Ok(None),
                    },
                };
                Ok(Some(e.insert(loaded)))
            }
        }
    }

    /// Writes the changes since the last save as one more chunk, compacting
    /// once enough have built up.
    fn persist(&self, id: Uuid, loaded: &mut Loaded) -> Result<(), SyncError> {
        let bytes = loaded.doc.save_after(&loaded.saved);
        if bytes.is_empty() {
            return Ok(());
        }
        self.store.append(id, loaded.kind, &bytes)?;
        loaded.saved = loaded.doc.get_heads();
        loaded.chunks += 1;
        if loaded.chunks > COMPACT_AFTER {
            self.store.compact(id, &loaded.doc.save())?;
            loaded.chunks = 1;
        }
        Ok(())
    }

    fn tombstoned(&self, docs: &mut HashMap<Uuid, Loaded>, id: Uuid) -> Result<bool, SyncError> {
        let Some(team) = self.load(docs, TEAM_DOC, None)? else {
            return Ok(false);
        };
        Ok(match map_at(&team.doc, "deleted") {
            Some(deleted) => team
                .doc
                .get(&deleted, id.to_string())
                .map_err(|e| SyncError::Corrupt(e.to_string()))?
                .is_some(),
            None => false,
        })
    }

    /// Records `id` in the team index and sets or clears its tombstone.
    /// Returns whether the team document changed.
    fn set_index(
        &self,
        docs: &mut HashMap<Uuid, Loaded>,
        id: Uuid,
        kind: DocKind,
        deleted: bool,
    ) -> Result<bool, SyncError> {
        let team = self
            .load(docs, TEAM_DOC, Some(DocKind::Team))?
            .expect("created when missing");
        let key = id.to_string();
        let before = team.doc.get_heads();
        team.doc
            .transact::<_, _, AutomergeError>(|tx| {
                let index = map_at_or_put(tx, "index")?;
                let tombstones = map_at_or_put(tx, "deleted")?;
                if tx.get(&index, &key)?.is_none() {
                    tx.put(&index, &key, kind.as_str())?;
                }
                let tombstoned = tx.get(&tombstones, &key)?.is_some();
                if deleted && !tombstoned {
                    tx.put(&tombstones, &key, true)?;
                } else if !deleted && tombstoned {
                    tx.delete(&tombstones, &key)?;
                }
                Ok(())
            })
            .map_err(|f| SyncError::Corrupt(format!("team document: {}", f.error)))?;
        let changed = team.doc.get_heads() != before;
        if changed {
            self.persist(TEAM_DOC, team)?;
        }
        Ok(changed)
    }

    fn notify(&self, kind: DocKind, id: Uuid, remote: bool) {
        let event = DocChanged { kind, id, remote };
        self.subscribers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|tx| tx.send(event).is_ok());
    }
}

fn check_kind(id: Uuid, stored: DocKind, wanted: DocKind) -> Result<(), SyncError> {
    if stored == wanted {
        Ok(())
    } else {
        Err(SyncError::Corrupt(format!(
            "document {id} is a {}, not a {}",
            stored.as_str(),
            wanted.as_str()
        )))
    }
}

/// The team document's first change, made the same way on every device: a
/// fixed actor, time zero, the same operations. Every copy therefore has
/// the same change hash, and the `index` and `deleted` maps of two devices
/// that started apart merge instead of conflicting.
fn team_skeleton() -> Automerge {
    let mut doc = Automerge::new().with_actor(ActorId::from(b"lampstand-team".as_slice()));
    doc.transact_with::<_, _, AutomergeError, _>(
        |_| CommitOptions::default().with_time(0),
        |tx| {
            tx.put_object(ROOT, "index", ObjType::Map)?;
            tx.put_object(ROOT, "deleted", ObjType::Map)?;
            Ok(())
        },
    )
    .expect("building an empty map cannot fail");
    doc
}

fn map_at(doc: &impl ReadDoc, key: &str) -> Option<ObjId> {
    match doc.get(ROOT, key) {
        Ok(Some((Value::Object(ObjType::Map), id))) => Some(id),
        _ => None,
    }
}

fn map_at_or_put(tx: &mut impl Transactable, key: &str) -> Result<ObjId, AutomergeError> {
    match map_at(tx, key) {
        Some(id) => Ok(id),
        None => tx.put_object(ROOT, key, ObjType::Map),
    }
}
