//! First-run content: two public-domain hymns, so a fresh install always
//! has something to show. They go through the OpenLyrics importer like any
//! other file. A marker file records that seeding happened, so songs the
//! operator deletes do not come back.

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

/// Imports the seed hymns unless this data folder was seeded before.
pub fn seed_if_new(library: &Library, data_dir: &Path) -> std::io::Result<()> {
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
