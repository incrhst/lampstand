//! EasyWorship 6 (`Songs.DB`, Paradox) and 7 (`Songs.db` SQLite, RTF lyrics). OpenLP's `easyworship.py` (GPL) is a reference for both.

use std::path::Path;

use crate::{ImportError, SongImport};

pub fn import(path: &Path) -> Result<SongImport, ImportError> {
    let _ = path;
    todo!("T09")
}
