//! First-run content: two public-domain hymns and the bundled
//! public-domain Bibles (KJV, WEB), so a fresh install always has songs and
//! scripture to show. The hymns go through the OpenLyrics importer like any
//! other file. A marker file for each records that seeding happened, so
//! songs and Bibles the operator deletes do not come back. The Bibles have
//! their own marker so that libraries seeded before they were bundled get
//! them too.

use std::path::Path;

use lampstand_core::scripture::parse_reference;
use lampstand_core::service::{ItemKind, Service};
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

/// The service a brand-new install starts with, so there is something to go
/// live with before anything has been planned: a heading that says how, the
/// two hymns, and a verse. `first_run` is whether this data folder had never
/// been seeded when the app started (it is not true of an existing install,
/// which is left as it is). A marker records that it happened, so deleting
/// the service does not bring it back.
pub fn seed_welcome_service(
    library: &Library,
    data_dir: &Path,
    first_run: bool,
) -> std::io::Result<()> {
    let marker = data_dir.join("seeded-welcome");
    if marker.exists() {
        return Ok(());
    }
    if first_run {
        if let Some(service) = welcome_service(library) {
            library
                .put_service(&service)
                .map_err(|e| std::io::Error::other(format!("adding the welcome service: {e:?}")))?;
        }
    }
    std::fs::write(marker, b"")
}

/// The welcome service, or `None` if the hymns or a Bible it uses are not in
/// the library (then there is nothing worth showing).
fn welcome_service(library: &Library) -> Option<Service> {
    let songs = library.songs().ok()?;
    let song = |title: &str| songs.iter().find(|s| s.title == title).map(|s| s.id);
    let grace = song("Amazing Grace")?;
    let holy = song("Holy, Holy, Holy")?;
    let translations = library.translations().ok()?;
    let bible = translations
        .iter()
        .find(|t| t.abbreviation == "KJV")
        .or_else(|| translations.first())?;
    let verse = parse_reference("John 3:16-17").ok()?;

    let mut service = Service::new("Welcome to Lampstand");
    let kinds = [
        ItemKind::Header {
            title: "Welcome. Press Space to go live, and again to move on".into(),
        },
        ItemKind::Song {
            song: grace,
            arrangement: None,
        },
        ItemKind::Scripture {
            passage: verse,
            translation: bible.id,
        },
        ItemKind::Song {
            song: holy,
            arrangement: None,
        },
        ItemKind::Header {
            title: "That is a whole service. Plan your own under Plan".into(),
        },
    ];
    for (i, kind) in kinds.into_iter().enumerate() {
        service.insert(i, kind);
    }
    Some(service)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seeded() -> (tempfile::TempDir, Library) {
        let dir = tempfile::tempdir().unwrap();
        let key = lampstand_sync::team::DeviceKeypair::generate();
        let library = Library::open(&dir.path().join("library"), key).unwrap();
        seed_if_new(&library, dir.path()).unwrap();
        (dir, library)
    }

    fn titles(library: &Library) -> Vec<String> {
        library
            .services()
            .unwrap()
            .into_iter()
            .map(|s| s.title)
            .collect()
    }

    #[test]
    fn a_new_install_starts_with_a_welcome_service_of_two_hymns_and_a_verse() {
        let (dir, library) = seeded();
        seed_welcome_service(&library, dir.path(), true).unwrap();
        let services = library.services().unwrap();
        let [welcome] = services.as_slice() else {
            panic!("expected one service, found {}", services.len());
        };
        assert_eq!(welcome.title, "Welcome to Lampstand");
        let kinds: Vec<&ItemKind> = welcome
            .ordered()
            .into_iter()
            .map(|(_, i)| &i.kind)
            .collect();
        assert_eq!(kinds.len(), 5);
        assert!(matches!(kinds[0], ItemKind::Header { .. }));
        assert!(matches!(kinds[1], ItemKind::Song { .. }));
        assert!(matches!(kinds[2], ItemKind::Scripture { .. }));
        assert!(matches!(kinds[3], ItemKind::Song { .. }));
        assert!(matches!(kinds[4], ItemKind::Header { .. }));
    }

    #[test]
    fn the_welcome_service_is_added_once_and_stays_deleted() {
        let (dir, library) = seeded();
        seed_welcome_service(&library, dir.path(), true).unwrap();
        seed_welcome_service(&library, dir.path(), true).unwrap();
        assert_eq!(
            titles(&library),
            ["Welcome to Lampstand"],
            "not added twice"
        );

        let id = library.services().unwrap()[0].id;
        library.delete_service(id).unwrap();
        seed_welcome_service(&library, dir.path(), true).unwrap();
        assert!(
            titles(&library).is_empty(),
            "a deleted welcome service does not come back"
        );
    }

    #[test]
    fn an_install_that_already_existed_is_left_alone() {
        let (dir, library) = seeded();
        seed_welcome_service(&library, dir.path(), false).unwrap();
        assert!(titles(&library).is_empty());
        // And it is not added on a later start either.
        seed_welcome_service(&library, dir.path(), true).unwrap();
        assert!(titles(&library).is_empty());
    }
}
