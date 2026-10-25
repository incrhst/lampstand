//! First-run content: two public-domain hymns and the bundled
//! public-domain Bibles (KJV, WEB), so a fresh install always has songs and
//! scripture to show. The hymns go through the OpenLyrics importer like any
//! other file. A marker file for each records that seeding happened, so
//! songs and Bibles the operator deletes do not come back. The Bibles have
//! their own marker so that libraries seeded before they were bundled get
//! them too.

use std::path::Path;

use lampstand_core::scripture::parse_reference;
use lampstand_core::service::{ItemKind, Service, Sound, SoundStart};
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

/// Thirty seconds of the project's own intro music (from
/// `video/public/music`), faded in and out, so a new install has a sound to
/// try the Sound box with. AAC in an `.m4a`, as most music libraries keep it.
const WALK_IN: &[u8] = include_bytes!("../seed/walk-in-music.m4a");
/// Its length, which a test checks against the file.
const WALK_IN_MS: u32 = 30_000;

const WELCOME_TITLE: &str = "Welcome to Lampstand";

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
/// two hymns, a verse and a sound. `first_run` is whether this data folder
/// had never been seeded when the app started (it is not true of an existing
/// install, which is left as it is). A marker records that it happened, so
/// deleting the service does not bring it back.
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
        if let Some(mut service) = welcome_service(library) {
            add_welcome_sound(library, &mut service)?;
            library
                .put_service(&service)
                .map_err(|e| std::io::Error::other(format!("adding the welcome service: {e:?}")))?;
        }
    }
    std::fs::write(marker, b"")
}

/// `ServiceRequest::CreateSample`: a new copy of the welcome service, for an
/// install that never had it (older than the welcome service, or than its
/// sound) or deleted it. The hymns and Bibles it uses are put back first if
/// they were deleted; the seed markers are left as they are.
pub fn sample_service(library: &Library) -> Result<Service, String> {
    if welcome_service(library).is_none() {
        let dir = std::env::temp_dir().join(format!("lampstand-sample-{}", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let mut summary = Summary::default();
        for (name, xml) in HYMNS {
            let path = dir.join(name);
            std::fs::write(&path, xml).map_err(|e| e.to_string())?;
            import_path(library, &path, &mut summary, &|_, _| {});
        }
        let _ = std::fs::remove_dir_all(&dir);
        if library
            .translations()
            .map_err(|e| format!("{e:?}"))?
            .is_empty()
        {
            library
                .install_bundled_bibles()
                .map_err(|e| format!("installing the Bibles: {e:?}"))?;
        }
    }
    let mut service =
        welcome_service(library).ok_or("the sample service's hymns could not be added")?;
    add_welcome_sound(library, &mut service).map_err(|e| e.to_string())?;
    Ok(service)
}

/// Puts the walk-in music, under a heading that says how to play it, before
/// the welcome service's closing heading.
fn add_welcome_sound(library: &Library, service: &mut Service) -> std::io::Result<()> {
    let file = library
        .repo()
        .blobs()
        .put(WALK_IN)
        .map_err(|e| std::io::Error::other(format!("adding the welcome sound: {e:?}")))?;
    let at = service.items.len().saturating_sub(1);
    service.insert(
        at,
        ItemKind::Header {
            title: "Sound. In Present, press Play in the Sound box".into(),
        },
    );
    service.insert(
        at + 1,
        ItemKind::Sound {
            sound: Sound {
                name: "Walk-in music".into(),
                file,
                length_ms: Some(WALK_IN_MS),
                start: SoundStart::Waits,
                keep_off_stream: false,
            },
        },
    );
    Ok(())
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

    let mut service = Service::new(WELCOME_TITLE);
    // Three groups, to show what an order can do: a section that loops by
    // itself (turn Loop off in its box to hold it), a plain service section
    // moved on with Space, and a closing header. Fold a group with its name.
    let kinds = [
        ItemKind::Header {
            title: "Pre-service loop. Its slides repeat on their own".into(),
        },
        ItemKind::Song {
            song: holy,
            arrangement: None,
        },
        ItemKind::Scripture {
            passage: verse,
            translation: bible.id,
        },
        ItemKind::Header {
            title: "Service. Press Space to go live, and again to move on".into(),
        },
        ItemKind::Song {
            song: grace,
            arrangement: None,
        },
        ItemKind::Header {
            title: "That is a whole service. Plan your own under Plan".into(),
        },
    ];
    let mut ids = Vec::new();
    for (i, kind) in kinds.into_iter().enumerate() {
        ids.push(service.insert(i, kind));
    }
    if let Some(item) = service.items.get_mut(&ids[0]) {
        item.loop_seconds = Some(6);
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

    fn sounds(service: &Service) -> Vec<Sound> {
        service
            .ordered()
            .into_iter()
            .filter_map(|(_, i)| match &i.kind {
                ItemKind::Sound { sound } => Some(sound.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_new_install_starts_with_a_welcome_service_of_two_hymns_a_verse_and_a_sound() {
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
        assert_eq!(kinds.len(), 8);
        assert!(matches!(kinds[0], ItemKind::Header { .. }));
        assert!(matches!(kinds[1], ItemKind::Song { .. }));
        assert!(matches!(kinds[2], ItemKind::Scripture { .. }));
        assert!(matches!(kinds[3], ItemKind::Header { .. }));
        assert!(matches!(kinds[4], ItemKind::Song { .. }));
        assert!(matches!(kinds[5], ItemKind::Header { .. }));
        assert!(matches!(kinds[6], ItemKind::Sound { .. }));
        assert!(
            matches!(kinds[7], ItemKind::Header { .. }),
            "it still ends with the closing heading"
        );
        let first = welcome.ordered()[0].1;
        assert_eq!(first.loop_seconds, Some(6), "the first group loops");

        let [sound] = sounds(welcome).try_into().unwrap();
        assert_eq!(sound.start, SoundStart::Waits);
        let stored = library
            .repo()
            .blobs()
            .path(&sound.file)
            .expect("in the library");
        assert_eq!(std::fs::read(stored).unwrap(), WALK_IN);
    }

    #[test]
    fn the_welcome_sound_plays_and_is_as_long_as_it_says() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("walk-in-music.m4a");
        std::fs::write(&path, WALK_IN).unwrap();
        let seconds = lampstand_media::sound_length(&path)
            .expect("readable")
            .expect("it gives its length");
        assert!(
            (seconds - f64::from(WALK_IN_MS) / 1000.0).abs() < 0.1,
            "{seconds} s"
        );
        let pcm = lampstand_media::load_sound(&path).unwrap();
        assert_eq!((pcm.rate, pcm.channels), (48_000, 2));
        let loudest = pcm.samples.iter().fold(0f32, |m, s| m.max(s.abs()));
        assert!(loudest > 0.1, "there is music, not silence ({loudest})");
    }

    #[test]
    fn the_sample_service_is_the_welcome_service_with_its_sound() {
        let (dir, library) = seeded();
        // An install from before the welcome service: nothing was added.
        seed_welcome_service(&library, dir.path(), false).unwrap();
        let sample = sample_service(&library).unwrap();
        assert_eq!(sample.title, WELCOME_TITLE);
        assert_eq!(sample.items.len(), 8);
        assert_eq!(sounds(&sample).len(), 1);
        // Asked for again, it is another copy, not the same service.
        assert_ne!(sample_service(&library).unwrap().id, sample.id);
    }

    #[test]
    fn the_sample_service_puts_deleted_hymns_back() {
        let (_dir, library) = seeded();
        for song in library.songs().unwrap() {
            library.delete_song(song.id).unwrap();
        }
        let sample = sample_service(&library).unwrap();
        let songs = sample
            .ordered()
            .into_iter()
            .filter(|(_, i)| matches!(i.kind, ItemKind::Song { .. }))
            .count();
        assert_eq!(songs, 2);
        assert_eq!(library.songs().unwrap().len(), 2);
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
