//! FreeShow `.show` JSON files (and the projects folder they live in).

use std::path::Path;

use crate::{ImportError, SongImport};

pub fn import(path: &Path) -> Result<SongImport, ImportError> {
    let _ = path;
    todo!("T08")
}
