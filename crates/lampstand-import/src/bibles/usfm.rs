//! USFM Bibles: a folder of one `.usfm` / `.SFM` file per book.

use std::path::Path;

use crate::{BibleImport, ImportError};

pub fn import(path: &Path) -> Result<BibleImport, ImportError> {
    let _ = path;
    todo!("T12")
}
