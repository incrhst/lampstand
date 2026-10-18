//! OpenLP 2.x and 3.x: `songs.sqlite`, lyrics stored as OpenLyrics XML in the `songs.lyrics` column.

use std::path::Path;

use crate::{ImportError, SongImport};

pub fn import(path: &Path) -> Result<SongImport, ImportError> {
    let _ = path;
    todo!("T06")
}
