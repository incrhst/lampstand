//! Documents on disk: `docs/<uuid>/kind` names the document's kind and
//! `docs/<uuid>/<hash>.chunk` holds its saved chunks. A chunk is either a
//! run of incremental changes or a whole-document snapshot. Loading applies
//! them all, in any order; Automerge ignores changes it already has.
//!
//! A chunk that will not load (cut short, or damaged on disk) is skipped
//! and renamed to `<hash>.chunk.corrupt`, so the rest of the document
//! still loads and compaction does not delete the bad bytes. When no chunk
//! loads, the document is damaged rather than empty: loading fails and the
//! chunks are left in place. Leftover `.tmp-*` files from an interrupted
//! write are ignored.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use automerge::{Automerge, Change, LoadOptions, OnPartialLoad, VerificationMode};
use uuid::Uuid;

use super::fsutil::{write_atomic, Durable};
use crate::{DocKind, SyncError};

const CHUNK_EXT: &str = "chunk";
const CORRUPT_EXT: &str = "corrupt";

pub(crate) struct DocStore {
    dir: PathBuf,
}

pub(crate) struct Loaded {
    pub kind: DocKind,
    pub doc: Automerge,
    pub chunks: usize,
}

impl DocStore {
    pub fn open(dir: PathBuf) -> Result<Self, SyncError> {
        fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    fn doc_dir(&self, id: Uuid) -> PathBuf {
        self.dir.join(id.to_string())
    }

    /// Every stored document with its kind.
    pub fn ids(&self) -> Result<Vec<(Uuid, DocKind)>, SyncError> {
        let mut out = Vec::new();
        for entry in fs::read_dir(&self.dir)? {
            let entry = entry?;
            let Ok(id) = entry.file_name().to_string_lossy().parse::<Uuid>() else {
                continue;
            };
            if let Some(kind) = self.kind(id)? {
                out.push((id, kind));
            }
        }
        Ok(out)
    }

    pub fn kind(&self, id: Uuid) -> Result<Option<DocKind>, SyncError> {
        match fs::read_to_string(self.doc_dir(id).join("kind")) {
            Ok(s) => DocKind::parse(s.trim())
                .map(Some)
                .ok_or_else(|| SyncError::Corrupt(format!("document {id} has kind {s:?}"))),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn load(&self, id: Uuid) -> Result<Option<Loaded>, SyncError> {
        let Some(kind) = self.kind(id)? else {
            return Ok(None);
        };
        let mut doc = Automerge::new();
        let mut chunks = 0;
        let mut bad = Vec::new();
        for path in self.chunk_paths(id)? {
            let bytes = fs::read(&path)?;
            let was_empty = doc.is_empty();
            let loaded = match doc.load_incremental(&bytes) {
                // Into a document that already has changes, Automerge keeps
                // whatever part of a chunk parses and says nothing, so a
                // chunk that added nothing is either a duplicate or garbage.
                // Parse it again on its own, strictly, to tell which.
                Ok(0) if !was_empty => check_alone(&bytes),
                Ok(_) => Ok(()),
                Err(e) => Err(e.to_string()),
            };
            match loaded {
                Ok(()) => chunks += 1,
                Err(e) => bad.push((path, e)),
            }
        }
        // Nothing loaded: the document is damaged, not empty. Its chunks
        // stay as they are, so it can be listed as damaged and deleted.
        if chunks == 0 {
            if let Some((path, e)) = bad.into_iter().next() {
                return Err(SyncError::Corrupt(format!("{}: {e}", path.display())));
            }
        } else {
            for (path, e) in bad {
                set_aside(&path, &e);
            }
        }
        Ok(Some(Loaded { kind, doc, chunks }))
    }

    /// Saves one more chunk of `id`'s changes. Durable once `durable` is
    /// finished.
    pub fn append(
        &self,
        id: Uuid,
        kind: DocKind,
        bytes: &[u8],
        durable: &mut Durable,
    ) -> Result<(), SyncError> {
        let dir = self.doc_dir(id);
        let kind_path = dir.join("kind");
        if !kind_path.exists() {
            fs::create_dir_all(&dir)?;
            durable.write_atomic(&kind_path, kind.as_str().as_bytes())?;
        }
        durable.write_atomic(&dir.join(chunk_name(bytes)), bytes)?;
        Ok(())
    }

    /// Replaces every chunk of `id` with one snapshot. The snapshot is in
    /// place before the old chunks go, so a crash part-way loses nothing.
    pub fn compact(&self, id: Uuid, snapshot: &[u8]) -> Result<(), SyncError> {
        let name = chunk_name(snapshot);
        let old = self.chunk_paths(id)?;
        write_atomic(&self.doc_dir(id).join(&name), snapshot)?;
        for path in old {
            if path.file_name().is_some_and(|n| n != name.as_str()) {
                fs::remove_file(path)?;
            }
        }
        Ok(())
    }

    fn chunk_paths(&self, id: Uuid) -> Result<Vec<PathBuf>, SyncError> {
        let dir = self.doc_dir(id);
        let mut paths = Vec::new();
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            // `.tmp-*` files have no extension, so a leftover one is skipped.
            if path.extension().is_some_and(|e| e == CHUNK_EXT) {
                paths.push(path);
            }
        }
        Ok(paths)
    }
}

/// Whether every Automerge chunk in `bytes` parses on its own. A file
/// holds one or more chunks, each `magic(4) checksum(4) type(1)
/// length(uleb128) body`. A document chunk is self-contained and is loaded
/// strictly; a change chunk is parsed without its dependencies.
fn check_alone(bytes: &[u8]) -> Result<(), String> {
    let bad = |why: &str| why.to_string();
    let mut rest = bytes;
    if rest.is_empty() {
        return Err(bad("empty chunk file"));
    }
    while !rest.is_empty() {
        let header = rest.get(..9).ok_or_else(|| bad("cut-off chunk header"))?;
        if header[..4] != MAGIC {
            return Err(bad("not an Automerge chunk"));
        }
        let (len, leb) = uleb128(&rest[9..]).ok_or_else(|| bad("bad chunk length"))?;
        let end = usize::try_from(len)
            .ok()
            .and_then(|len| (9 + leb).checked_add(len))
            .filter(|&end| end <= rest.len())
            .ok_or_else(|| bad("chunk cut short"))?;
        let (chunk, tail) = rest.split_at(end);
        if header[8] == DOCUMENT_CHUNK {
            Automerge::load_with_options(
                chunk,
                LoadOptions::new()
                    .on_partial_load(OnPartialLoad::Error)
                    .verification_mode(VerificationMode::DontCheck),
            )
            .map_err(|e| bad(&e.to_string()))?;
        } else {
            Change::try_from(chunk).map_err(|e| bad(&e.to_string()))?;
        }
        rest = tail;
    }
    Ok(())
}

const MAGIC: [u8; 4] = [0x85, 0x6f, 0x4a, 0x83];
const DOCUMENT_CHUNK: u8 = 0;

/// An unsigned LEB128 number and how many bytes it took.
fn uleb128(bytes: &[u8]) -> Option<(u64, usize)> {
    let mut value = 0u64;
    for (i, &b) in bytes.iter().enumerate().take(10) {
        value |= u64::from(b & 0x7f) << (7 * i);
        if b & 0x80 == 0 {
            return Some((value, i + 1));
        }
    }
    None
}

/// Reports a chunk that will not load and renames it out of the way, so
/// later loads do not trip on it again and compaction leaves it alone.
fn set_aside(path: &Path, why: &str) {
    let aside = path.with_extension(format!("{CHUNK_EXT}.{CORRUPT_EXT}"));
    eprintln!(
        "lampstand: skipped a saved chunk that will not load ({why}); kept as {}",
        aside.display()
    );
    if let Err(e) = fs::rename(path, &aside) {
        eprintln!("lampstand: cannot set aside {}: {e}", path.display());
    }
}

fn chunk_name(bytes: &[u8]) -> String {
    let hash = blake3::hash(bytes).to_hex();
    format!("{}.{CHUNK_EXT}", &hash[..32])
}

#[cfg(test)]
mod tests {
    use super::*;

    use automerge::transaction::Transactable;
    use automerge::{ReadDoc, ROOT};

    fn append(store: &DocStore, id: Uuid, doc: &mut Automerge, key: &str) {
        let heads = doc.get_heads();
        let mut tx = doc.transaction();
        tx.put(ROOT, key, "yes").unwrap();
        tx.commit();
        let bytes = doc.save_after(&heads);
        let mut durable = Durable::default();
        store
            .append(id, DocKind::Service, &bytes, &mut durable)
            .unwrap();
        durable.finish().unwrap();
    }

    fn keys(doc: &Automerge) -> Vec<String> {
        doc.keys(ROOT).collect()
    }

    fn files(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_corrupt_chunk_does_not_lose_the_document() {
        let dir = tempfile::tempdir().unwrap();
        let store = DocStore::open(dir.path().to_path_buf()).unwrap();
        let id = Uuid::from_u128(7);
        let mut doc = Automerge::new();
        append(&store, id, &mut doc, "a");
        append(&store, id, &mut doc, "b");
        // A chunk damaged on disk: an Automerge header, then garbage.
        let bad = [0x85, 0x6f, 0x4a, 0x83, 1, 2, 3, 4, 5, 6, 7, 8];
        let bad_path = store.doc_dir(id).join(chunk_name(&bad));
        fs::write(&bad_path, bad).unwrap();
        // One cut short by a crash. Automerge keeps whatever of it parses,
        // so it loads rather than being set aside; either way it must not
        // fail the load.
        let mut torn = doc.save();
        torn.truncate(torn.len() / 2);
        fs::write(store.doc_dir(id).join(chunk_name(&torn)), &torn).unwrap();

        let loaded = store.load(id).unwrap().unwrap();
        assert_eq!(keys(&loaded.doc), ["a", "b"]);
        assert!(!bad_path.exists(), "the bad chunk is set aside");
        let aside = bad_path.with_extension("chunk.corrupt");
        assert_eq!(fs::read(&aside).unwrap(), bad, "and kept for a closer look");

        // It loads again, and compaction leaves the set-aside bytes alone.
        let loaded = store.load(id).unwrap().unwrap();
        assert_eq!(keys(&loaded.doc), ["a", "b"]);
        store.compact(id, &loaded.doc.save()).unwrap();
        let names = files(&store.doc_dir(id));
        assert_eq!(names.iter().filter(|n| n.ends_with(".chunk")).count(), 1);
        assert!(aside.exists());
        assert_eq!(keys(&store.load(id).unwrap().unwrap().doc), ["a", "b"]);
    }

    #[test]
    fn a_chunk_already_covered_by_a_snapshot_is_not_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        let store = DocStore::open(dir.path().to_path_buf()).unwrap();
        let id = Uuid::from_u128(7);
        let mut doc = Automerge::new();
        append(&store, id, &mut doc, "a");
        append(&store, id, &mut doc, "b");
        // A compaction cut short: the snapshot is in, the old chunks remain.
        let snapshot = doc.save();
        fs::write(store.doc_dir(id).join(chunk_name(&snapshot)), &snapshot).unwrap();

        let loaded = store.load(id).unwrap().unwrap();
        assert_eq!(keys(&loaded.doc), ["a", "b"]);
        assert_eq!(loaded.chunks, 3);
        let names = files(&store.doc_dir(id));
        assert!(!names.iter().any(|n| n.ends_with(".corrupt")), "{names:?}");
    }

    #[test]
    fn a_document_with_no_readable_chunk_is_damaged_not_empty() {
        let dir = tempfile::tempdir().unwrap();
        let store = DocStore::open(dir.path().to_path_buf()).unwrap();
        let id = Uuid::from_u128(7);
        let mut doc = Automerge::new();
        append(&store, id, &mut doc, "a");
        append(&store, id, &mut doc, "b");
        for path in store.chunk_paths(id).unwrap() {
            fs::write(&path, b"this is not an automerge chunk").unwrap();
        }
        let before = files(&store.doc_dir(id));

        assert!(matches!(store.load(id), Err(SyncError::Corrupt(_))));
        assert_eq!(files(&store.doc_dir(id)), before, "nothing is set aside");
        assert_eq!(store.ids().unwrap(), [(id, DocKind::Service)]);
    }

    #[test]
    fn a_stray_temp_file_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let store = DocStore::open(dir.path().to_path_buf()).unwrap();
        let id = Uuid::from_u128(7);
        let mut doc = Automerge::new();
        append(&store, id, &mut doc, "a");
        // Left behind by a write the power cut interrupted.
        let tmp = store.doc_dir(id).join(".tmp-0123456789abcdef");
        fs::write(&tmp, b"half a chunk").unwrap();

        let loaded = store.load(id).unwrap().unwrap();
        assert_eq!(keys(&loaded.doc), ["a"]);
        assert_eq!(loaded.chunks, 1);
        assert!(tmp.exists(), "loading only reads");
        assert_eq!(store.ids().unwrap(), [(id, DocKind::Service)]);
    }
}
