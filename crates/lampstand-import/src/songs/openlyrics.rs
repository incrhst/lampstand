//! OpenLyrics 0.9 XML, one song per file or a folder of them. Also used by OpenLP's exports and by T06.

use std::path::Path;

use crate::{ImportError, SongImport};

pub fn import(path: &Path) -> Result<SongImport, ImportError> {
    let _ = path;
    todo!("T04")
}
