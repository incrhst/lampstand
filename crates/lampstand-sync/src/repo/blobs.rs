use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use lampstand_core::id::BlobHash;

use super::fsutil::TempFile;
use crate::SyncError;

/// Content-addressed files. Writes are atomic (temp file then rename) and
/// verified against their hash.
///
/// Layout: `dir/<64 hex digits>`, with temp files (`.tmp-*`) beside them.
pub struct BlobStore {
    dir: PathBuf,
}

impl BlobStore {
    pub(crate) fn open(dir: PathBuf) -> Result<Self, SyncError> {
        fs::create_dir_all(&dir)?;
        // Temp files left by a crash mid-write.
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            if entry.file_name().to_string_lossy().starts_with(".tmp-") {
                let _ = fs::remove_file(entry.path());
            }
        }
        Ok(Self { dir })
    }

    pub fn put(&self, bytes: &[u8]) -> Result<BlobHash, SyncError> {
        self.write(bytes, None)
    }

    /// Writes a blob fetched from a peer, refusing it unless it hashes to
    /// `expected`. Streams, so a video never has to fit in memory.
    pub fn put_verified(&self, expected: &BlobHash, reader: impl Read) -> Result<(), SyncError> {
        self.write(reader, Some(expected)).map(|_| ())
    }

    /// Copies a file into the store, for example a dropped-in video.
    pub fn import(&self, source: &Path) -> Result<BlobHash, SyncError> {
        self.write(File::open(source)?, None)
    }

    pub fn path(&self, hash: &BlobHash) -> Option<PathBuf> {
        let path = self.path_of(hash);
        path.is_file().then_some(path)
    }

    /// The hashes in `wanted` this device does not have yet.
    pub fn missing(&self, wanted: &[BlobHash]) -> Vec<BlobHash> {
        wanted
            .iter()
            .filter(|h| self.path(h).is_none())
            .copied()
            .collect()
    }

    fn path_of(&self, hash: &BlobHash) -> PathBuf {
        self.dir.join(hash.to_string())
    }

    fn write(
        &self,
        mut reader: impl Read,
        expected: Option<&BlobHash>,
    ) -> Result<BlobHash, SyncError> {
        let mut tmp = TempFile::new_in(&self.dir)?;
        let mut hasher = blake3::Hasher::new();
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e.into()),
            };
            hasher.update(&buf[..n]);
            tmp.file().write_all(&buf[..n])?;
        }
        let hash = BlobHash::from_bytes(*hasher.finalize().as_bytes());
        if let Some(expected) = expected {
            if *expected != hash {
                return Err(SyncError::Corrupt(format!(
                    "blob {expected} arrived with hash {hash}"
                )));
            }
        }
        let target = self.path_of(&hash);
        if !target.is_file() {
            tmp.persist(&target)?;
        }
        Ok(hash)
    }
}
