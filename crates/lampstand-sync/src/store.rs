//! CD-1: Automerge documents on disk.
//!
//! Layout of a store directory:
//!
//! ```text
//! <dir>/actor                               this device's Automerge actor id (hex)
//! <dir>/docs/library.automerge              compacted snapshot, a plain Automerge file
//! <dir>/docs/library.log                    changes since the snapshot
//! <dir>/docs/services/2026-10-18.automerge
//! ```
//!
//! Every edit or sync message appends its new changes to the document's log
//! as one record (a 4-byte big-endian length, then an incremental Automerge
//! save), which stays cheap even for a large library. Opening the store folds
//! each log into a fresh snapshot. A crash part-way through an append leaves a
//! short last record, which loading drops; those changes come back on the
//! next sync. (Automerge's own partial-load option cannot be used for this:
//! it drops every change after the snapshot, not just the torn one.)

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use automerge::sync::{self, SyncDoc};
use automerge::transaction::CommitOptions;
use automerge::{ActorId, AutoCommit, AutomergeError, ChangeHash};

use crate::{DocId, SyncError};

const DOCS_DIR: &str = "docs";
const ACTOR_FILE: &str = "actor";

/// One change in a document's history, newest last.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryEntry {
    pub hash: ChangeHash,
    /// Device that made the change (Automerge actor id, hex).
    pub actor: String,
    /// Position of this change among that device's changes, from 1.
    pub seq: u64,
    /// Milliseconds since the Unix epoch, as set by the device that made the
    /// change. Clocks differ between devices, so use it for display only.
    pub time: i64,
    pub message: Option<String>,
}

/// Every Automerge document this device holds, kept in memory and on disk.
pub struct Store {
    dir: PathBuf,
    actor: ActorId,
    docs: BTreeMap<DocId, AutoCommit>,
}

impl Store {
    /// Opens the store in `dir`, creating it if needed, and loads every
    /// document in it.
    pub fn open(dir: impl Into<PathBuf>) -> Result<Self, SyncError> {
        let dir = dir.into();
        fs::create_dir_all(dir.join(DOCS_DIR))?;
        let actor = load_or_create_actor(&dir.join(ACTOR_FILE))?;
        let mut store = Self {
            dir,
            actor,
            docs: BTreeMap::new(),
        };
        let docs_dir = store.dir.join(DOCS_DIR);
        for (id, path) in find_docs(&docs_dir)? {
            let mut doc = AutoCommit::load(&fs::read(&path)?)?;
            let log = path.with_extension("log");
            if let Ok(bytes) = fs::read(&log) {
                for record in log_records(&bytes) {
                    doc.load_incremental(record)?;
                }
                write_atomically(&path, &doc.save())?;
                // A crash before this line only means the log is replayed
                // again next time, which Automerge ignores as duplicates.
                fs::remove_file(&log)?;
            } else {
                // Moves the incremental-save cursor to the end.
                doc.save();
            }
            doc.set_actor(store.actor.clone());
            store.docs.insert(id, doc);
        }
        Ok(store)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// This device's Automerge actor id, in hex. Also used as its peer id.
    pub fn actor_hex(&self) -> String {
        self.actor.to_hex_string()
    }

    pub fn doc_ids(&self) -> Vec<DocId> {
        self.docs.keys().cloned().collect()
    }

    pub fn contains(&self, id: &DocId) -> bool {
        self.docs.contains_key(id)
    }

    /// The document, for reading. Write through [`Store::edit`] so the
    /// change is committed and saved.
    pub fn doc(&mut self, id: &DocId) -> Option<&mut AutoCommit> {
        self.docs.get_mut(id)
    }

    /// Creates a document, or adds its starting structure to one that
    /// already exists.
    ///
    /// `init` builds the starting structure, such as an empty `songs` map.
    /// It runs under a fixed actor id and timestamp derived from `id`, so
    /// every device that creates the same document with the same `init`
    /// produces a byte-identical first change. When two devices create a
    /// document offline and later sync, Automerge sees one shared first
    /// change instead of two rival `songs` maps, one of which would hide the
    /// other's songs. `init` must therefore be deterministic.
    pub fn create(
        &mut self,
        id: &DocId,
        init: impl FnOnce(&mut AutoCommit) -> Result<(), AutomergeError>,
    ) -> Result<(), SyncError> {
        let mut seed = AutoCommit::new()
            .with_actor(ActorId::from(format!("lampstand-create:{id}").as_bytes()));
        init(&mut seed)?;
        seed.commit_with(
            CommitOptions::default()
                .with_time(0)
                .with_message(format!("create {id}")),
        );
        let changes = seed.get_changes(&[]);
        let doc = self.ensure(id);
        doc.apply_changes(changes)?;
        self.persist(id)
    }

    /// Returns the document, creating an empty one with no changes if this
    /// store has never seen it. Used when a peer offers a document.
    pub fn ensure(&mut self, id: &DocId) -> &mut AutoCommit {
        let actor = self.actor.clone();
        self.docs
            .entry(id.clone())
            .or_insert_with(|| AutoCommit::new().with_actor(actor))
    }

    /// Makes a local edit, commits it as one change with `message` and saves
    /// it. If `f` fails, its partial edits are rolled back.
    pub fn edit<R>(
        &mut self,
        id: &DocId,
        message: &str,
        f: impl FnOnce(&mut AutoCommit) -> Result<R, AutomergeError>,
    ) -> Result<R, SyncError> {
        let doc = self
            .docs
            .get_mut(id)
            .ok_or_else(|| SyncError::NoSuchDoc(id.to_string()))?;
        let result = match f(doc) {
            Ok(result) => result,
            Err(e) => {
                doc.rollback();
                return Err(e.into());
            }
        };
        doc.commit_with(
            CommitOptions::default()
                .with_time(now_millis())
                .with_message(message),
        );
        self.persist(id)?;
        Ok(result)
    }

    /// Merges every change in `other` that this store's copy lacks, and
    /// saves. Returns true if anything new arrived.
    pub fn merge(&mut self, id: &DocId, other: &mut AutoCommit) -> Result<bool, SyncError> {
        let added = self.ensure(id).merge(other)?;
        self.persist(id)?;
        Ok(!added.is_empty())
    }

    /// The document's change history in causal order, oldest first.
    pub fn history(&mut self, id: &DocId) -> Result<Vec<HistoryEntry>, SyncError> {
        let doc = self
            .docs
            .get_mut(id)
            .ok_or_else(|| SyncError::NoSuchDoc(id.to_string()))?;
        Ok(doc
            .get_changes(&[])
            .iter()
            .map(|change| HistoryEntry {
                hash: change.hash(),
                actor: change.actor_id().to_hex_string(),
                seq: change.seq(),
                time: change.timestamp(),
                message: change.message().map(str::to_owned),
            })
            .collect())
    }

    pub fn heads(&mut self, id: &DocId) -> Vec<ChangeHash> {
        self.docs
            .get_mut(id)
            .map(AutoCommit::get_heads)
            .unwrap_or_default()
    }

    /// Next Automerge sync message to send to a peer for this document, if
    /// any. Creates an empty document if needed so we can ask the peer for
    /// one we do not have yet.
    pub fn generate_sync_message(
        &mut self,
        id: &DocId,
        state: &mut sync::State,
    ) -> Option<sync::Message> {
        self.ensure(id).sync().generate_sync_message(state)
    }

    /// Applies a sync message from a peer and saves. Returns true if the
    /// document changed.
    pub fn receive_sync_message(
        &mut self,
        id: &DocId,
        state: &mut sync::State,
        message: sync::Message,
    ) -> Result<bool, SyncError> {
        let doc = self.ensure(id);
        let before = doc.get_heads();
        doc.sync().receive_sync_message(state, message)?;
        let changed = doc.get_heads() != before;
        if changed {
            self.persist(id)?;
        }
        Ok(changed)
    }

    /// Writes changes made since the last save to disk.
    fn persist(&mut self, id: &DocId) -> Result<(), SyncError> {
        let path = self.dir.join(DOCS_DIR).join(id.relative_path());
        let doc = self
            .docs
            .get_mut(id)
            .ok_or_else(|| SyncError::NoSuchDoc(id.to_string()))?;
        if !path.exists() {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            write_atomically(&path, &doc.save())?;
            return Ok(());
        }
        let chunk = doc.save_incremental();
        if !chunk.is_empty() {
            let mut record = Vec::with_capacity(4 + chunk.len());
            record.extend_from_slice(&(chunk.len() as u32).to_be_bytes());
            record.extend_from_slice(&chunk);
            let mut log = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path.with_extension("log"))?;
            log.write_all(&record)?;
            log.sync_data()?;
        }
        Ok(())
    }
}

/// Complete records of a log file, dropping a torn last record.
fn log_records(mut bytes: &[u8]) -> Vec<&[u8]> {
    let mut records = Vec::new();
    while bytes.len() >= 4 {
        let len = u32::from_be_bytes(bytes[..4].try_into().unwrap()) as usize;
        let Some(record) = bytes.get(4..4 + len) else {
            break;
        };
        records.push(record);
        bytes = &bytes[4 + len..];
    }
    records
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn load_or_create_actor(path: &Path) -> Result<ActorId, SyncError> {
    if let Ok(hex) = fs::read_to_string(path) {
        if let Ok(actor) = ActorId::try_from(hex.trim()) {
            return Ok(actor);
        }
    }
    let actor = ActorId::random();
    write_atomically(path, actor.to_hex_string().as_bytes())?;
    Ok(actor)
}

fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), SyncError> {
    let tmp = path.with_extension("tmp");
    let mut file = File::create(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&tmp, path)?;
    Ok(())
}

/// Every `*.automerge` file under `docs_dir`, with the document id its path
/// spells. Files whose path is not a valid id are skipped.
fn find_docs(docs_dir: &Path) -> Result<Vec<(DocId, PathBuf)>, SyncError> {
    let mut found = Vec::new();
    let mut pending = vec![docs_dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir)? {
            let path = entry?.path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("automerge") {
                continue;
            }
            let Ok(relative) = path
                .with_extension("")
                .strip_prefix(docs_dir)
                .map(Path::to_path_buf)
            else {
                continue;
            };
            let segments: Option<Vec<&str>> = relative.iter().map(|s| s.to_str()).collect();
            if let Some(Ok(id)) = segments.map(|s| DocId::new(s.join("/"))) {
                found.push((id, path));
            }
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use automerge::transaction::Transactable;
    use automerge::{ObjType, ReadDoc, ROOT};

    fn library() -> DocId {
        DocId::new("library").unwrap()
    }

    fn init_library(doc: &mut AutoCommit) -> Result<(), AutomergeError> {
        doc.put_object(ROOT, "songs", ObjType::Map)?;
        Ok(())
    }

    fn add_song(store: &mut Store, key: &str, title: &str) {
        store
            .edit(&library(), &format!("add {title}"), |doc| {
                let (_, songs) = doc.get(ROOT, "songs")?.expect("songs map");
                let song = doc.put_object(&songs, key, ObjType::Map)?;
                doc.put(&song, "title", title)?;
                Ok(())
            })
            .unwrap();
    }

    fn song_titles(store: &mut Store) -> Vec<String> {
        let doc = store.doc(&library()).unwrap();
        let (_, songs) = doc.get(ROOT, "songs").unwrap().unwrap();
        doc.keys(&songs)
            .map(|key| {
                let (_, song) = doc.get(&songs, key.as_str()).unwrap().unwrap();
                doc.get(&song, "title")
                    .unwrap()
                    .unwrap()
                    .0
                    .into_string()
                    .unwrap()
            })
            .collect()
    }

    #[test]
    fn edits_survive_reopen_with_history() {
        let dir = tempfile::tempdir().unwrap();
        let actor = {
            let mut store = Store::open(dir.path()).unwrap();
            store.create(&library(), init_library).unwrap();
            add_song(&mut store, "s1", "Amazing Grace");
            add_song(&mut store, "s2", "Be Thou My Vision");
            store.actor_hex()
        };
        let mut store = Store::open(dir.path()).unwrap();
        assert_eq!(store.actor_hex(), actor, "device keeps its actor id");
        assert_eq!(
            song_titles(&mut store),
            ["Amazing Grace", "Be Thou My Vision"]
        );
        let history = store.history(&library()).unwrap();
        let messages: Vec<_> = history.iter().map(|h| h.message.as_deref()).collect();
        assert_eq!(
            messages,
            [
                Some("create library"),
                Some("add Amazing Grace"),
                Some("add Be Thou My Vision")
            ]
        );
        assert_eq!(history[1].actor, actor);
    }

    #[test]
    fn offline_replicas_merge_without_conflict() {
        let (dir_a, dir_b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let mut a = Store::open(dir_a.path()).unwrap();
        let mut b = Store::open(dir_b.path()).unwrap();

        // Both devices create the library while offline, then each adds a song
        // and both edit the same song's title.
        a.create(&library(), init_library).unwrap();
        b.create(&library(), init_library).unwrap();
        add_song(&mut a, "s1", "Amazing Grace");
        add_song(&mut b, "s2", "Be Thou My Vision");

        let mut b_doc = b.doc(&library()).unwrap().fork();
        assert!(a.merge(&library(), &mut b_doc).unwrap());
        let mut a_doc = a.doc(&library()).unwrap().fork();
        assert!(b.merge(&library(), &mut a_doc).unwrap());

        let mut titles = song_titles(&mut a);
        titles.sort();
        assert_eq!(titles, ["Amazing Grace", "Be Thou My Vision"]);
        assert_eq!(song_titles(&mut a), song_titles(&mut b));
        assert_eq!(a.heads(&library()), b.heads(&library()));
        // Shared first change, then one change from each device.
        assert_eq!(a.history(&library()).unwrap().len(), 3);

        // The merge was saved on both sides.
        drop((a, b));
        let mut b = Store::open(dir_b.path()).unwrap();
        assert_eq!(song_titles(&mut b).len(), 2);
    }

    #[test]
    fn torn_append_is_ignored_on_load() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut store = Store::open(dir.path()).unwrap();
            store.create(&library(), init_library).unwrap();
            add_song(&mut store, "s1", "Amazing Grace");
        }
        let log = dir.path().join("docs/library.log");
        assert!(log.exists(), "edits go to the log");
        let mut file = OpenOptions::new().append(true).open(&log).unwrap();
        // A record that claims 100 bytes but was cut short by a crash.
        file.write_all(&[0, 0, 0, 100, 0x85, 0x6f, 0x4a]).unwrap();
        drop(file);
        let mut store = Store::open(dir.path()).unwrap();
        assert_eq!(song_titles(&mut store), ["Amazing Grace"]);
        assert!(!log.exists(), "opening folds the log into the snapshot");
        add_song(&mut store, "s2", "Be Thou My Vision");
        drop(store);
        let mut store = Store::open(dir.path()).unwrap();
        assert_eq!(song_titles(&mut store).len(), 2);
    }

    #[test]
    fn nested_doc_ids_round_trip_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let id = DocId::new("services/2026-10-18-am").unwrap();
        Store::open(dir.path())
            .unwrap()
            .create(&id, |doc| doc.put(ROOT, "title", "Sunday AM"))
            .unwrap();
        let store = Store::open(dir.path()).unwrap();
        assert_eq!(store.doc_ids(), [id]);
    }
}
