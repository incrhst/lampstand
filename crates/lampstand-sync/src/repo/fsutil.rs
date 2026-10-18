//! Atomic file writes: a temp file in the target's directory, flushed to
//! disk, then renamed over the target.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// A temp file beside `dir`'s other files, removed on drop unless it was
/// renamed into place with [`TempFile::persist`].
pub(crate) struct TempFile {
    path: PathBuf,
    file: Option<File>,
}

impl TempFile {
    pub(crate) fn new_in(dir: &Path) -> io::Result<Self> {
        let path = dir.join(format!(".tmp-{}", random_hex()));
        let file = File::options().write(true).create_new(true).open(&path)?;
        Ok(Self {
            path,
            file: Some(file),
        })
    }

    pub(crate) fn file(&mut self) -> &mut File {
        self.file.as_mut().expect("temp file open until persisted")
    }

    /// Flushes the contents to disk and renames the file to `target`.
    pub(crate) fn persist(mut self, target: &Path) -> io::Result<()> {
        let file = self.file.take().expect("temp file open until persisted");
        file.sync_all()?;
        drop(file);
        fs::rename(&self.path, target)?;
        sync_dir(target.parent().unwrap_or(Path::new(".")));
        Ok(())
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if self.file.take().is_some() {
            let _ = fs::remove_file(&self.path);
        }
    }
}

pub(crate) fn write_atomic(target: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = target.parent().unwrap_or(Path::new("."));
    let mut tmp = TempFile::new_in(dir)?;
    tmp.file().write_all(bytes)?;
    tmp.persist(target)
}

/// Makes a rename durable on Unix. Windows has no directory handles to
/// flush, and NTFS journals the rename itself.
fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    if let Ok(d) = File::open(dir) {
        let _ = d.sync_all();
    }
    #[cfg(not(unix))]
    let _ = dir;
}

pub(crate) fn random_bytes<const N: usize>() -> [u8; N] {
    let mut buf = [0u8; N];
    getrandom::fill(&mut buf).expect("operating system random source");
    buf
}

fn random_hex() -> String {
    random_bytes::<8>()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
