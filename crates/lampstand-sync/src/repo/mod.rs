//! The repo: documents in memory and on disk, the team document's index,
//! per-peer sync state and the blob store.

mod blobs;
mod fsutil;
mod store;

use std::collections::hash_map::Entry as MapEntry;
use std::collections::{BTreeSet, HashMap, HashSet};
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
#[cfg(feature = "net")]
pub(crate) use fsutil::{random_bytes, write_atomic};

use crate::team::{DeviceKey, DeviceKeypair};
use crate::{Doc, DocChanged, DocKind, SyncError};
use fsutil::Durable;
use store::DocStore;

/// The team document's ID until T18 gives each team its own.
const TEAM_DOC: Uuid = Uuid::nil();

/// Incremental chunks a document collects on disk before it is rewritten
/// as one snapshot.
const COMPACT_AFTER: usize = 32;

/// Bytes of randomness after the device key in an actor ID.
const ACTOR_TAIL: usize = 8;

/// Documents one peer may have offered that this device has not got yet
/// (see [`Unknown`]). The first sync offers the whole library at once, and
/// the peer's changes start arriving only after this device has answered
/// the lot, so every offer of a new device's first sync waits at the same
/// time. A church library can pass 25,000 documents (5,000 songs, a whole
/// song database downloaded, services, decks); this leaves room for more
/// than twice that. A flood past it is refused.
pub const MAX_UNKNOWN_DOCS: usize = 60_000;

/// Memory those offers from one peer may take, counted as each message's
/// size plus [`UNKNOWN_OFFER_OVERHEAD`]: the sync state kept grows with the
/// message (its heads and bloom filter), so a count alone would not bound
/// it. A typical offer is charged about 1 KiB, so this binds at about the
/// same place as [`MAX_UNKNOWN_DOCS`], and earlier for larger messages.
pub const MAX_UNKNOWN_BYTES: usize = 64 * 1024 * 1024;

/// What a waiting offer is charged beyond its message's size: its sync
/// state and map entries. Measured at 800 to 910 bytes for a song's
/// opening message of 49 bytes (`tests/offer_memory.rs`, which checks
/// that this covers it).
pub const UNKNOWN_OFFER_OVERHEAD: usize = 1024;

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
    /// Tests: make the next chunk write fail.
    #[cfg(test)]
    fail_next_write: std::sync::atomic::AtomicBool,
}

#[derive(Default)]
struct State {
    docs: HashMap<Uuid, Loaded>,
    peers: HashMap<(DeviceKey, Uuid), sync::State>,
    unknown: HashMap<DeviceKey, Unknown>,
    /// Documents whose changes from a peer could not be written: nothing
    /// more about them passes with that peer until it reconnects (see
    /// [`Repo::forget_unsaved`]).
    stalled: HashSet<(DeviceKey, Uuid)>,
}

/// The documents a peer has offered that this device does not have: only
/// the kind the peer claims, so the reply can carry it. No document exists
/// for them, in memory or on disk, and nothing fixes their kind, until a
/// message brings a change. Bounded per peer, and dropped with the peer's
/// sync state ([`Repo::forget_peer`]).
#[derive(Default)]
struct Unknown {
    /// Each ID's claimed kind and the size of the last message about it.
    docs: HashMap<Uuid, (DocKind, usize)>,
    bytes: usize,
}

impl Unknown {
    fn remove(&mut self, id: Uuid) {
        if let Some((_, size)) = self.docs.remove(&id) {
            self.bytes -= size;
        }
    }

    /// Records (or updates) the claim, unless that would pass the bounds.
    fn claim(&mut self, id: Uuid, kind: DocKind, size: usize) -> bool {
        let (count, bytes) = match self.docs.get(&id) {
            Some((_, old)) => (self.docs.len(), self.bytes - old),
            None => (self.docs.len() + 1, self.bytes),
        };
        if count > MAX_UNKNOWN_DOCS || bytes + size > MAX_UNKNOWN_BYTES {
            return false;
        }
        self.bytes = bytes + size;
        self.docs.insert(id, (kind, size));
        true
    }
}

/// Chunks written together, counted as saved only once all of them are
/// durable ([`Repo::commit`]). Until then a document's `saved` heads, and
/// any sync state that says a peer's changes arrived, stay where they were.
#[derive(Default)]
struct Batch {
    durable: Durable,
    /// Each document written, with the heads its chunk saved.
    written: Vec<(Uuid, Vec<ChangeHash>)>,
}

/// A document in memory, and how much of it is on disk. Outside a write in
/// progress the document holds nothing that is not on disk: a write that
/// fails drops it from memory ([`Repo::forget_unsaved`]), so it is loaded
/// again as it is on disk.
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
            #[cfg(test)]
            fail_next_write: Default::default(),
        };
        {
            let mut st = repo.lock();
            if repo.load(&mut st.docs, TEAM_DOC, None)?.is_none() {
                let doc = team_skeleton().with_actor(repo.actor.clone());
                let team = Loaded {
                    kind: DocKind::Team,
                    doc,
                    saved: Vec::new(),
                    chunks: 0,
                };
                let mut batch = Batch::default();
                repo.persist(TEAM_DOC, &team, &mut batch)?;
                st.docs.insert(TEAM_DOC, team);
                if let Err(e) = repo.commit(&mut st.docs, batch) {
                    st.docs.remove(&TEAM_DOC);
                    return Err(e);
                }
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
            .map_err(|e| SyncError::Unreadable(format!("{} {id}: {e}", T::KIND.as_str())))
    }

    /// Writes `doc`, diffing against the stored document so that only the
    /// fields that changed become CRDT operations. If the write fails, the
    /// document is left as it is on disk.
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
        let mut batch = Batch::default();
        let written = (|| {
            if changed {
                let loaded = st.docs.get(&id).expect("loaded above");
                self.persist(id, loaded, &mut batch)?;
            }
            let indexed = self.set_index(&mut st.docs, id, T::KIND, false, &mut batch)?;
            self.commit(&mut st.docs, batch)?;
            Ok(indexed)
        })();
        let indexed = match written {
            Ok(indexed) => indexed,
            Err(e) => {
                self.forget_unsaved(&mut st, &[id, TEAM_DOC], None);
                return Err(e);
            }
        };
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
        let mut batch = Batch::default();
        let written = self
            .set_index(&mut st.docs, id, kind, true, &mut batch)
            .and_then(|changed| self.commit(&mut st.docs, batch).map(|()| changed));
        let changed = match written {
            Ok(changed) => changed,
            Err(e) => {
                self.forget_unsaved(&mut st, &[TEAM_DOC], None);
                return Err(e);
            }
        };
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
    /// `None`, unless `peer` offered them: then the reply asks for them.
    pub fn generate_sync_message(&self, peer: DeviceKey, id: Uuid) -> Option<SyncMessage> {
        let mut st = self.lock();
        let State {
            docs,
            peers,
            unknown,
            stalled,
        } = &mut *st;
        if stalled.contains(&(peer, id)) {
            return None;
        }
        let (kind, message) = match self.load(docs, id, None).ok()? {
            Some(loaded) => {
                let state = peers.entry((peer, id)).or_default();
                (loaded.kind, loaded.doc.generate_sync_message(state))
            }
            None => {
                let (kind, _) = *unknown.get(&peer)?.docs.get(&id)?;
                let state = peers.entry((peer, id)).or_default();
                (kind, Automerge::new().generate_sync_message(state))
            }
        };
        message.map(|m| SyncMessage {
            kind,
            id,
            data: m.encode(),
        })
    }

    /// Applies a sync message from `peer`. A document this device has not
    /// seen is created only once a message brings a change (see
    /// [`MAX_UNKNOWN_DOCS`]).
    ///
    /// Changes count as received only once they are durable. If writing
    /// them fails, the document is dropped from memory and so is `peer`'s
    /// sync state for it: this device then says it has only what is on
    /// disk, and the peer sends the changes again on the next connection.
    pub fn receive_sync_message(
        &self,
        peer: DeviceKey,
        message: SyncMessage,
    ) -> Result<(), SyncError> {
        let (kind, id) = (message.kind, message.id);
        let mut st = self.lock();
        let changed = self.apply(&mut st, peer, message)?;
        if changed {
            let mut batch = Batch::default();
            let loaded = st.docs.get(&id).expect("applied above");
            let written = self
                .persist(id, loaded, &mut batch)
                .and_then(|()| self.commit(&mut st.docs, batch));
            if let Err(e) = written {
                self.forget_unsaved(&mut st, &[id], Some(peer));
                return Err(e);
            }
        }
        drop(st);
        if changed {
            self.notify(kind, id, true);
        }
        Ok(())
    }

    /// Applies many sync messages from `peer` at once: each changed
    /// document is written once for the whole batch, rather than once per
    /// message. Results are per message, in order. A first sync of a large
    /// library arrives this way. A document whose write fails is handled as
    /// in [`Repo::receive_sync_message`], and its messages' results say so.
    pub fn receive_sync_messages(
        &self,
        peer: DeviceKey,
        messages: Vec<SyncMessage>,
    ) -> Vec<(Uuid, Result<(), SyncError>)> {
        let mut results = Vec::with_capacity(messages.len());
        let mut changed: Vec<(DocKind, Uuid)> = Vec::new();
        let mut st = self.lock();
        for message in messages {
            let (kind, id) = (message.kind, message.id);
            let result = self.apply(&mut st, peer, message).map(|applied| {
                if applied && !changed.contains(&(kind, id)) {
                    changed.push((kind, id));
                }
            });
            results.push((id, result));
        }
        let mut persisted = Vec::with_capacity(changed.len());
        let mut failed: Vec<(Uuid, String)> = Vec::new();
        let mut batch = Batch::default();
        for (kind, id) in changed {
            // A later message about it may have failed and dropped it.
            let Some(loaded) = st.docs.get(&id) else {
                failed.push((id, "a later message failed".into()));
                continue;
            };
            match self.persist(id, loaded, &mut batch) {
                Ok(()) => persisted.push((kind, id)),
                Err(e) => failed.push((id, e.to_string())),
            }
        }
        if let Err(e) = self.commit(&mut st.docs, batch) {
            let why = e.to_string();
            failed.extend(persisted.drain(..).map(|(_, id)| (id, why.clone())));
        }
        for (id, why) in &failed {
            self.forget_unsaved(&mut st, &[*id], Some(peer));
            for (rid, r) in results.iter_mut() {
                if rid == id && r.is_ok() {
                    *r = Err(SyncError::Corrupt(format!("{id} not saved: {why}")));
                }
            }
        }
        drop(st);
        for (kind, id) in persisted {
            self.notify(kind, id, true);
        }
        results
    }

    /// The team document's ID: offer it with every new document, as a new
    /// document changes the team index too.
    pub fn team_doc(&self) -> Uuid {
        TEAM_DOC
    }

    /// Drops the sync state kept for `peer`, when its connection closes,
    /// with the documents it offered that this device never got. The next
    /// connection starts the sync handshake afresh.
    pub fn forget_peer(&self, peer: DeviceKey) {
        let mut st = self.lock();
        st.peers.retain(|(p, _), _| *p != peer);
        st.unknown.remove(&peer);
        st.stalled.retain(|(p, _)| *p != peer);
    }

    /// Applies one sync message from `peer` in memory. Returns whether the
    /// document changed, and so needs writing.
    ///
    /// A message about a document this device has not got creates nothing
    /// unless it brings a change: until then only the peer's sync state and
    /// claimed kind are kept, within [`MAX_UNKNOWN_DOCS`] and
    /// [`MAX_UNKNOWN_BYTES`], so a peer can neither fill memory with empty
    /// documents nor fix a document's kind with a message that has no
    /// changes.
    fn apply(
        &self,
        st: &mut State,
        peer: DeviceKey,
        message: SyncMessage,
    ) -> Result<bool, SyncError> {
        let SyncMessage { kind, id, data } = message;
        let corrupt =
            |e: &dyn std::fmt::Display| SyncError::Corrupt(format!("sync message for {id}: {e}"));
        let decoded = sync::Message::decode(&data).map_err(|e| corrupt(&e))?;
        let State {
            docs,
            peers,
            unknown,
            stalled,
        } = st;
        if stalled.contains(&(peer, id)) {
            return Ok(false);
        }
        if let Some(loaded) = self.load(docs, id, None)? {
            if let Some(offered) = unknown.get_mut(&peer) {
                offered.remove(id);
            }
            check_kind(id, loaded.kind, kind)?;
            // T18: drop changes whose author's role may not write `kind`.
            let before = loaded.doc.get_heads();
            let received = loaded
                .doc
                .receive_sync_message(peers.entry((peer, id)).or_default(), decoded);
            let changed = loaded.doc.get_heads() != before;
            if let Err(e) = received {
                if changed {
                    // Part of it applied, and cannot be kept: handle it as
                    // a failed write (see `forget_unsaved`).
                    docs.remove(&id);
                    peers.remove(&(peer, id));
                    stalled.insert((peer, id));
                }
                return Err(corrupt(&e));
            }
            return Ok(changed);
        }
        // Not here: try the message on an empty document and a copy of the
        // peer's sync state, and keep them only if it brought a change or
        // the peer is within its bounds.
        let mut doc = Automerge::new().with_actor(self.actor.clone());
        let mut state = peers.get(&(peer, id)).cloned().unwrap_or_default();
        // T18: drop changes whose author's role may not write `kind`.
        doc.receive_sync_message(&mut state, decoded)
            .map_err(|e| corrupt(&e))?;
        if !doc.get_heads().is_empty() {
            for offered in unknown.values_mut() {
                offered.remove(id);
            }
            peers.insert((peer, id), state);
            docs.insert(
                id,
                Loaded {
                    kind,
                    doc,
                    saved: Vec::new(),
                    chunks: 0,
                },
            );
            return Ok(true);
        }
        if !unknown
            .entry(peer)
            .or_default()
            .claim(id, kind, data.len() + UNKNOWN_OFFER_OVERHEAD)
        {
            peers.remove(&(peer, id));
            return Err(SyncError::Corrupt(format!(
                "sync message for {id}: the peer has offered more documents this device \
                 does not have than it holds at once"
            )));
        }
        peers.insert((peer, id), state);
        Ok(false)
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

    /// Writes the changes since the last save as one more chunk of `batch`.
    /// They count as saved once the batch is committed.
    fn persist(&self, id: Uuid, loaded: &Loaded, batch: &mut Batch) -> Result<(), SyncError> {
        let bytes = loaded.doc.save_after(&loaded.saved);
        if bytes.is_empty() {
            return Ok(());
        }
        #[cfg(test)]
        if self
            .fail_next_write
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            return Err(std::io::Error::other("injected write failure").into());
        }
        self.store
            .append(id, loaded.kind, &bytes, &mut batch.durable)?;
        batch.written.push((id, loaded.doc.get_heads()));
        Ok(())
    }

    /// Makes `batch` durable, and only then records its chunks as saved,
    /// compacting a document once enough have built up. On an error nothing
    /// is recorded, and the caller drops the batch's documents from memory
    /// ([`Repo::forget_unsaved`]).
    fn commit(&self, docs: &mut HashMap<Uuid, Loaded>, batch: Batch) -> Result<(), SyncError> {
        let Batch { durable, written } = batch;
        durable.finish()?;
        for (id, heads) in written {
            let Some(loaded) = docs.get_mut(&id) else {
                continue;
            };
            loaded.saved = heads;
            loaded.chunks += 1;
            if loaded.chunks > COMPACT_AFTER {
                // The chunks are durable already; a failed compaction is
                // tried again on the next write.
                match self.store.compact(id, &loaded.doc.save()) {
                    Ok(()) => loaded.chunks = 1,
                    Err(e) => eprintln!("lampstand: compacting {id} failed: {e}"),
                }
            }
        }
        Ok(())
    }

    /// After a failed write: drops `ids` from memory, so they load again as
    /// they are on disk, and `peer`'s sync state for them, so its changes
    /// that did not reach the disk are asked for again on the next
    /// connection rather than counted as received.
    ///
    /// Until then the documents are *stalled* with `peer`: its messages
    /// about them are ignored and none are sent. The peer remembers having
    /// sent those changes and will not send them again in this session, so
    /// carrying on would only have the two ask and answer each other
    /// forever. [`Repo::forget_peer`] ends the stall.
    fn forget_unsaved(&self, st: &mut State, ids: &[Uuid], peer: Option<DeviceKey>) {
        for id in ids {
            st.docs.remove(id);
            if let Some(peer) = peer {
                st.peers.remove(&(peer, *id));
                st.stalled.insert((peer, *id));
            }
        }
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
        batch: &mut Batch,
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
            self.persist(TEAM_DOC, team, batch)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use lampstand_core::song::Song;

    fn open(dir: &std::path::Path) -> Repo {
        Repo::open(dir.to_path_buf(), DeviceKeypair::generate()).unwrap()
    }

    /// A peer's opening message about a document with one change: its heads
    /// and a bloom filter, no changes.
    fn opening_message() -> Vec<u8> {
        let mut doc = Automerge::new();
        doc.transact::<_, _, AutomergeError>(|tx| tx.put(ROOT, "title", "x").map(|_| ()))
            .unwrap();
        doc.generate_sync_message(&mut sync::State::new())
            .unwrap()
            .encode()
    }

    fn unknown(repo: &Repo, peer: DeviceKey) -> (usize, usize) {
        let st = repo.lock();
        st.unknown
            .get(&peer)
            .map_or((0, 0), |u| (u.docs.len(), u.bytes))
    }

    #[test]
    fn a_flood_of_unknown_ids_creates_no_documents_and_is_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let repo = open(dir.path());
        let peer = DeviceKeypair::generate().public();
        let data = opening_message();
        let flood: Vec<SyncMessage> = (0..MAX_UNKNOWN_DOCS + 50)
            .map(|_| SyncMessage {
                kind: DocKind::Song,
                id: Uuid::now_v7(),
                data: data.clone(),
            })
            .collect();
        let results = repo.receive_sync_messages(peer, flood);
        let refused = results.iter().filter(|(_, r)| r.is_err()).count();
        assert_eq!(refused, 50);
        assert!(results[..MAX_UNKNOWN_DOCS].iter().all(|(_, r)| r.is_ok()));
        // Nothing but the team document, in memory or on disk.
        assert_eq!(repo.lock().docs.len(), 1);
        assert_eq!(repo.documents(), vec![(DocKind::Team, TEAM_DOC)]);
        assert_eq!(fs_entries(&dir.path().join("docs")), 1);
        assert_eq!(
            unknown(&repo, peer),
            (
                MAX_UNKNOWN_DOCS,
                MAX_UNKNOWN_DOCS * (data.len() + UNKNOWN_OFFER_OVERHEAD)
            )
        );
        assert_eq!(repo.lock().peers.len(), MAX_UNKNOWN_DOCS);
        // Each accepted one is answered, so a real offer still syncs.
        let (_, first) = &results[0];
        assert!(first.is_ok());
        let id = results[0].0;
        assert!(repo.generate_sync_message(peer, id).is_some());
        // A refused one is not.
        let refused_id = results.last().unwrap().0;
        assert!(repo.generate_sync_message(peer, refused_id).is_none());
        // Another peer has bounds of its own.
        let other = DeviceKeypair::generate().public();
        repo.receive_sync_message(
            other,
            SyncMessage {
                kind: DocKind::Song,
                id: refused_id,
                data: data.clone(),
            },
        )
        .unwrap();
        // Disconnecting frees them.
        repo.forget_peer(peer);
        assert_eq!(unknown(&repo, peer), (0, 0));
        assert_eq!(repo.lock().peers.len(), 1);
    }

    #[test]
    fn unknown_documents_are_bounded_by_bytes_too() {
        let mut u = Unknown::default();
        let (a, b) = (Uuid::now_v7(), Uuid::now_v7());
        assert!(u.claim(a, DocKind::Song, MAX_UNKNOWN_BYTES - 10));
        assert!(!u.claim(b, DocKind::Song, 11));
        assert!(u.claim(b, DocKind::Song, 10));
        // Restating a claim replaces its size rather than adding to it.
        assert!(u.claim(a, DocKind::Song, MAX_UNKNOWN_BYTES - 10));
        assert_eq!(u.bytes, MAX_UNKNOWN_BYTES);
        u.remove(a);
        assert_eq!((u.docs.len(), u.bytes), (1, 10));
    }

    /// Passes sync messages both ways until neither side has anything to
    /// say, returning the errors `b` met on the way.
    fn sync(a: &Repo, b: &Repo) -> Vec<SyncError> {
        let mut errors = Vec::new();
        for _ in 0..20 {
            let mut ids: Vec<Uuid> = a.documents().into_iter().map(|(_, id)| id).collect();
            ids.extend(b.documents().into_iter().map(|(_, id)| id));
            ids.sort();
            ids.dedup();
            let mut quiet = true;
            for id in ids {
                if let Some(m) = a.generate_sync_message(b.device(), id) {
                    for (_, r) in b.receive_sync_messages(a.device(), vec![m]) {
                        errors.extend(r.err());
                    }
                    quiet = false;
                }
                if let Some(m) = b.generate_sync_message(a.device(), id) {
                    a.receive_sync_message(b.device(), m).unwrap();
                    quiet = false;
                }
            }
            if quiet {
                return errors;
            }
        }
        panic!("repos did not converge in 20 rounds");
    }

    fn reconnect(a: &Repo, b: &Repo) {
        a.forget_peer(b.device());
        b.forget_peer(a.device());
    }

    fn title(repo: &Repo, id: Uuid) -> Option<String> {
        repo.get::<Song>(id).unwrap().map(|s| s.title)
    }

    fn fail_next_write(repo: &Repo) {
        repo.fail_next_write
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    #[test]
    fn changes_whose_write_failed_arrive_again_on_the_next_connect() {
        let (da, db) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (a, b) = (open(da.path()), open(db.path()));
        let mut song = Song::new("Amazing Grace");
        let id = song.id.uuid();
        a.put(&song).unwrap();
        assert!(sync(&a, &b).is_empty());
        assert_eq!(title(&b, id).as_deref(), Some("Amazing Grace"));

        // An edit reaches b, and b fails to write it.
        song.title = "Amazing Grace (My Chains Are Gone)".into();
        a.put(&song).unwrap();
        fail_next_write(&b);
        let errors = sync(&a, &b);
        assert_eq!(errors.len(), 1, "{errors:?}");
        // b holds, and so tells a it has, only what is on disk.
        assert_eq!(title(&b, id).as_deref(), Some("Amazing Grace"));
        assert_eq!(
            title(&open(db.path()), id).as_deref(),
            Some("Amazing Grace")
        );

        // The next connection brings the edit again, and it is saved.
        reconnect(&a, &b);
        assert!(sync(&a, &b).is_empty());
        assert_eq!(title(&b, id), Some(song.title.clone()));
        drop(b);
        assert_eq!(title(&open(db.path()), id), Some(song.title));
    }

    #[test]
    fn a_new_document_whose_write_failed_arrives_again_on_the_next_connect() {
        let (da, db) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (a, b) = (open(da.path()), open(db.path()));
        assert!(sync(&a, &b).is_empty());
        let song = Song::new("Be Thou My Vision");
        let id = song.id.uuid();
        a.put(&song).unwrap();
        // Fail the write of the message that brings the song itself.
        let mut failed = false;
        for _ in 0..20 {
            for doc in [TEAM_DOC, id] {
                if let Some(m) = a.generate_sync_message(b.device(), doc) {
                    let brings_song = doc == id && !failed;
                    if brings_song {
                        fail_next_write(&b);
                    }
                    let results = b.receive_sync_messages(a.device(), vec![m]);
                    if brings_song && results.iter().any(|(_, r)| r.is_err()) {
                        failed = true;
                    } else {
                        // Not written: the flag is still set.
                        b.fail_next_write
                            .store(false, std::sync::atomic::Ordering::SeqCst);
                    }
                }
                if let Some(m) = b.generate_sync_message(a.device(), doc) {
                    a.receive_sync_message(b.device(), m).unwrap();
                }
            }
        }
        assert!(failed);
        assert_eq!(title(&b, id), None);
        assert!(!b.documents().iter().any(|(_, d)| *d == id));
        assert_eq!(title(&open(db.path()), id), None);

        reconnect(&a, &b);
        assert!(sync(&a, &b).is_empty());
        drop(b);
        assert_eq!(
            title(&open(db.path()), id).as_deref(),
            Some("Be Thou My Vision")
        );
    }

    #[test]
    fn a_put_whose_write_failed_leaves_the_document_as_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let repo = open(dir.path());
        let mut song = Song::new("Abide With Me");
        let id = song.id.uuid();
        repo.put(&song).unwrap();
        song.title = "Abide with Me".into();
        fail_next_write(&repo);
        assert!(repo.put(&song).is_err());
        assert_eq!(title(&repo, id).as_deref(), Some("Abide With Me"));
        // Trying again writes it.
        repo.put(&song).unwrap();
        drop(repo);
        assert_eq!(
            title(&open(dir.path()), id).as_deref(),
            Some("Abide with Me")
        );
    }

    fn fs_entries(dir: &std::path::Path) -> usize {
        std::fs::read_dir(dir).unwrap().count()
    }
}
