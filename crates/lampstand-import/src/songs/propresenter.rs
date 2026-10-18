//! ProPresenter 6 (`.pro6` XML, RTF in base64) and 7 (`.pro` protobuf, using the community proto definitions after checking their licence).

use std::path::Path;

use crate::{ImportError, SongImport};

pub fn import(path: &Path) -> Result<SongImport, ImportError> {
    let _ = path;
    todo!("T10")
}
