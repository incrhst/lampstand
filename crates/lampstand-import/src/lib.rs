//! Importers for other tools' song libraries and Bible formats, and the
//! migration wizard's search for them.
//!
//! PRD: SL-4, BI-2, Migration.
//!
//! Each format is one module with one `import` function, so that formats
//! can be built in parallel. Importers are best effort: a song they cannot
//! read is skipped with a reason, never a failed import. Fixtures live in
//! `tests/fixtures/<format>/`, each a real export (or a hand-made file in
//! the same shape, where real ones carry copyrighted lyrics).

pub mod bibles;
pub mod detect;
pub mod songs;

use std::path::{Path, PathBuf};

use lampstand_core::scripture::{BibleText, Translation};
use lampstand_core::song::Song;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SongFormat {
    OpenLyrics,
    OpenSong,
    OpenLp,
    SongSelect,
    FreeShow,
    EasyWorship,
    ProPresenter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BibleFormat {
    Osis,
    Usfm,
    Zefania,
    OpenSong,
    OpenLp,
}

#[derive(Debug, Default)]
pub struct SongImport {
    pub songs: Vec<Song>,
    /// Media the songs refer to (backgrounds), to add to the blob store.
    pub media: Vec<PathBuf>,
    pub skipped: Vec<Skipped>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    /// File name, or "file name: song title" inside a database.
    pub source: String,
    pub reason: String,
}

#[derive(Debug)]
pub struct BibleImport {
    /// `id` and `text` are filled in by the caller once stored.
    pub translation: Translation,
    pub text: BibleText,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub enum ImportError {
    Io(std::io::Error),
    /// Not this format at all.
    WrongFormat,
    Unreadable(String),
}

impl From<std::io::Error> for ImportError {
    fn from(e: std::io::Error) -> Self {
        ImportError::Io(e)
    }
}

/// `path` is a file or folder, whichever the format uses.
pub fn import_songs(format: SongFormat, path: &Path) -> Result<SongImport, ImportError> {
    use songs::*;
    match format {
        SongFormat::OpenLyrics => openlyrics::import(path),
        SongFormat::OpenSong => opensong::import(path),
        SongFormat::OpenLp => openlp::import(path),
        SongFormat::SongSelect => songselect::import(path),
        SongFormat::FreeShow => freeshow::import(path),
        SongFormat::EasyWorship => easyworship::import(path),
        SongFormat::ProPresenter => propresenter::import(path),
    }
}

pub fn import_bible(format: BibleFormat, path: &Path) -> Result<BibleImport, ImportError> {
    use bibles::*;
    match format {
        BibleFormat::Osis => osis::import(path),
        BibleFormat::Usfm => usfm::import(path),
        BibleFormat::Zefania => zefania::import(path),
        BibleFormat::OpenSong => opensong::import(path),
        BibleFormat::OpenLp => openlp::import(path),
    }
}
