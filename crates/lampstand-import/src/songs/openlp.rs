//! OpenLP 2.x and 3.x: `songs.sqlite`, lyrics stored as OpenLyrics XML in the `songs.lyrics` column.
//!
//! Not supported yet (T06 was skipped). OpenLP users can export their songs
//! as OpenLyrics from OpenLP and import those instead.

use std::path::Path;

use crate::{ImportError, SongImport};

pub fn import(path: &Path) -> Result<SongImport, ImportError> {
    let _ = path;
    Err(ImportError::Unreadable(
        "OpenLP databases are not supported yet; export the songs as OpenLyrics from OpenLP and import those".into(),
    ))
}
