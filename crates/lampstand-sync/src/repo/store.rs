//! Documents on disk: `docs/<uuid>/kind` names the document's kind and
//! `docs/<uuid>/<hash>.chunk` holds its saved chunks. A chunk is either a
//! run of incremental changes or a whole-document snapshot. Loading applies
//! them all, in any order; Automerge ignores changes it already has.

use std::fs;
use std::io;
use std::path::PathBuf;

use automerge::Automerge;
use uuid::Uuid;

use super::fsutil::{write_atomic, Durable};
use crate::{DocKind, SyncError};

const CHUNK_EXT: &str = "chunk";

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
        for path in self.chunk_paths(id)? {
            let bytes = fs::read(&path)?;
            doc.load_incremental(&bytes)
                .map_err(|e| SyncError::Corrupt(format!("{}: {e}", path.display())))?;
            chunks += 1;
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
            if path.extension().is_some_and(|e| e == CHUNK_EXT) {
                paths.push(path);
            }
        }
        Ok(paths)
    }
}

fn chunk_name(bytes: &[u8]) -> String {
    let hash = blake3::hash(bytes).to_hex();
    format!("{}.{CHUNK_EXT}", &hash[..32])
}
