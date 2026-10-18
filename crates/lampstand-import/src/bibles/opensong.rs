//! OpenSong Bibles (`<bible><b><c><v>` XML).

use std::path::Path;

use crate::{BibleImport, ImportError};

pub fn import(path: &Path) -> Result<BibleImport, ImportError> {
    let _ = path;
    todo!("T13")
}
