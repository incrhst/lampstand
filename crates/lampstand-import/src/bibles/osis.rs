//! OSIS XML Bibles (CrossWire, eBible.org).

use std::path::Path;

use crate::{BibleImport, ImportError};

pub fn import(path: &Path) -> Result<BibleImport, ImportError> {
    let _ = path;
    todo!("T11")
}
