//! First-run content: two public-domain hymns and the bundled
//! public-domain Bibles (KJV, WEB), so a fresh install always has songs and
//! scripture to show. The hymns go through the OpenLyrics importer like any
//! other file. A marker file for each records that seeding happened, so
//! songs and Bibles the operator deletes do not come back. The Bibles have
//! their own marker so that libraries seeded before they were bundled get
//! them too.

use std::path::Path;

use lampstand_library::Library;

use crate::import::{import_path, Summary};

const HYMNS: [(&str, &str); 2] = [
    (
        "amazing-grace.xml",
        include_str!("../seed/amazing-grace.xml"),
    ),
    (
        "holy-holy-holy.xml",
        include_str!("../seed/holy-holy-holy.xml"),
    ),
];

/// Imports the seed hymns and installs the bundled Bibles, each unless
/// this data folder had it seeded before.
pub fn seed_if_new(library: &Library, data_dir: &Path) -> std::io::Result<()> {
    seed_hymns(library, data_dir)?;
    seed_bibles(library, data_dir)
}

fn seed_hymns(library: &Library, data_dir: &Path) -> std::io::Result<()> {
    let marker = data_dir.join("seeded");
    if marker.exists() {
        return Ok(());
    }
    let dir = data_dir.join("seed");
    std::fs::create_dir_all(&dir)?;
    let mut summary = Summary::default();
    for (name, xml) in HYMNS {
        let path = dir.join(name);
        std::fs::write(&path, xml)?;
        import_path(library, &path, &mut summary, &|_, _| {});
    }
    if !summary.skipped.is_empty() {
        eprintln!("lampstand: seeding: {}", summary.message());
    }
    std::fs::remove_dir_all(&dir)?;
    std::fs::write(marker, b"")
}

fn seed_bibles(library: &Library, data_dir: &Path) -> std::io::Result<()> {
    let marker = data_dir.join("seeded-bibles");
    if marker.exists() {
        return Ok(());
    }
    library
        .install_bundled_bibles()
        .map_err(|e| std::io::Error::other(format!("installing bundled Bibles: {e:?}")))?;
    std::fs::write(marker, b"")
}
