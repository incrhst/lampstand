//! CCLI SongSelect downloads: `.usr` (key=value with `/t` section separators) and `.txt` lyrics files.

use std::path::Path;

use crate::{ImportError, SongImport};

pub fn import(path: &Path) -> Result<SongImport, ImportError> {
    let _ = path;
    todo!("T07")
}
