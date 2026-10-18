//! OpenSong songs: XML files without extension in a `Songs` folder, lyrics in OpenSong's `[V1]` / `.`chord-line text format.

use std::path::Path;

use crate::{ImportError, SongImport};

pub fn import(path: &Path) -> Result<SongImport, ImportError> {
    let _ = path;
    todo!("T05")
}
