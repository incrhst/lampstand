//! Finds other projection tools' data on this machine, for the first-run
//! migration wizard: default data folders per OS and per app version.

use std::path::PathBuf;

use crate::SongFormat;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub app: SongFormat,
    pub path: PathBuf,
    /// Approximate song count, if cheap to get.
    pub songs: Option<usize>,
}

pub fn installed() -> Vec<Found> {
    todo!("T55")
}
