//! Training services (#190): Training 0 to 2, bundled with the app and
//! built from `training/` at every start, so a fresh install has them,
//! offline, and an update brings the new ones.
//!
//! `training/services/*.json` say what is in each service; the decks
//! (`training/decks/*.pptx`) and videos (`training/media/*.mp4`) they name
//! are embedded here and put in the blob store, where the renderer finds
//! them like any other. The songs are the seeded public-domain hymns, parsed
//! again as training's own copies, and the scripture uses the bundled KJV
//! and WEB. `training/README.md` says how to rebuild the decks and videos.
//!
//! The services, decks and songs go to the library's training set
//! (`lampstand_library::training`), never to the repo: they are read-only
//! in Plan, have "Reset training", stay out of CCLI reports and never sync.
//! Every ID is fixed, so a service open for live when the app stopped comes
//! back on the same slide (the crash journal names items by ID).

use std::collections::BTreeMap;

use lampstand_core::deck::{Deck, DeckId, DeckSource, Version};
use lampstand_core::id::{BlobHash, Timestamp};
use lampstand_core::scripture::parse_reference;
use lampstand_core::service::{ItemId, ItemKind, Service, ServiceId};
use lampstand_core::song::{Song, SongId};
use lampstand_import::songs::openlyrics::parse_song;
use lampstand_library::bundled::BUNDLED;
use lampstand_library::training::TrainingSet;
use lampstand_library::Library;
use serde::Deserialize;
use uuid::Uuid;

/// The service definitions, Training 0 first.
pub const SERVICES: [&str; 3] = [
    include_str!("../../../training/services/training-0.json"),
    include_str!("../../../training/services/training-1.json"),
    include_str!("../../../training/services/training-2.json"),
];

/// Every deck and video a definition may name, by file name.
pub const FILES: &[(&str, &[u8])] = &[
    (
        "t0-plan-and-present.pptx",
        include_bytes!("../../../training/decks/t0-plan-and-present.pptx"),
    ),
    (
        "t1-present-screen.pptx",
        include_bytes!("../../../training/decks/t1-present-screen.pptx"),
    ),
    (
        "t1-sermon-practice.pptx",
        include_bytes!("../../../training/decks/t1-sermon-practice.pptx"),
    ),
    (
        "t1-ready.pptx",
        include_bytes!("../../../training/decks/t1-ready.pptx"),
    ),
    (
        "t2-briefing.pptx",
        include_bytes!("../../../training/decks/t2-briefing.pptx"),
    ),
    (
        "t2-sermon-thursday.pptx",
        include_bytes!("../../../training/decks/t2-sermon-thursday.pptx"),
    ),
    (
        "t2-sermon-sunday.pptx",
        include_bytes!("../../../training/decks/t2-sermon-sunday.pptx"),
    ),
    (
        "t2-debrief.pptx",
        include_bytes!("../../../training/decks/t2-debrief.pptx"),
    ),
    (
        "t0-welcome.mp4",
        include_bytes!("../../../training/media/t0-welcome.mp4"),
    ),
    (
        "t1-welcome.mp4",
        include_bytes!("../../../training/media/t1-welcome.mp4"),
    ),
    (
        "t1-going-live.mp4",
        include_bytes!("../../../training/media/t1-going-live.mp4"),
    ),
    (
        "t1-blank-idle-clear.mp4",
        include_bytes!("../../../training/media/t1-blank-idle-clear.mp4"),
    ),
    (
        "t2-five-surprises.mp4",
        include_bytes!("../../../training/media/t2-five-surprises.mp4"),
    ),
];

/// The hymns training uses, as training's own copies under fixed IDs: the
/// church may edit or delete its own.
const HYMNS: [(&str, u128, &str); 2] = [
    (
        "Amazing Grace",
        0x0192_0000_0000_7000_8000_7473_6f6e_6701,
        include_str!("../seed/amazing-grace.xml"),
    ),
    (
        "Holy, Holy, Holy",
        0x0192_0000_0000_7000_8000_7473_6f6e_6702,
        include_str!("../seed/holy-holy-holy.xml"),
    ),
];

/// One service in `training/services`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub id: Uuid,
    pub title: String,
    pub items: Vec<Entry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    #[serde(flatten)]
    pub item: Item,
    /// The trainer's notes, shown in Present beside the live picture.
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Item {
    Header {
        title: String,
    },
    /// A video in `training/media`; `name` must end in `.mp4`.
    Video {
        file: String,
        name: String,
    },
    /// A deck in `training/decks`.
    Deck {
        file: String,
        title: String,
    },
    /// One of the seeded hymns, by title.
    Song {
        title: String,
    },
    /// A passage in one of the bundled Bibles ("KJV", "WEB").
    Scripture {
        reference: String,
        translation: String,
    },
    Text {
        title: String,
        body: String,
    },
}

/// The definitions, parsed.
pub fn definitions() -> Result<Vec<Definition>, String> {
    SERVICES
        .iter()
        .enumerate()
        .map(|(i, json)| {
            serde_json::from_str(json).map_err(|e| format!("training service {i}: {e}"))
        })
        .collect()
}

fn file(name: &str) -> Result<&'static [u8], String> {
    FILES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, bytes)| *bytes)
        .ok_or_else(|| format!("no training file {name}"))
}

/// An ID made from a service's and a number, so every start gives the same.
fn derived(service: Uuid, n: u128) -> Uuid {
    Uuid::from_u128(service.as_u128().wrapping_add(n << 64))
}

/// Builds the training set, putting the files it uses in the blob store.
pub fn build(library: &Library) -> Result<TrainingSet, String> {
    let blobs = library.repo().blobs();
    let put = |bytes: &[u8]| -> Result<BlobHash, String> {
        let hash = BlobHash::of(bytes);
        if blobs.path(&hash).is_none() {
            blobs.put(bytes).map_err(|e| e.to_string())?;
        }
        Ok(hash)
    };

    let mut set = TrainingSet::default();
    for (title, id, xml) in HYMNS {
        let mut song = parse_song(xml).map_err(|e| format!("{title}: {e}"))?;
        song.id = SongId::from_uuid(Uuid::from_u128(id));
        set.songs.push(song);
    }
    for bible in &BUNDLED {
        put(bible.blob)?;
        set.translations.push(bible.translation());
    }

    for def in definitions()? {
        let mut service = Service::new(def.title.clone());
        service.id = ServiceId::from_uuid(def.id);
        let mut notes = Vec::new();
        for (n, entry) in def.items.iter().enumerate() {
            let kind = match &entry.item {
                Item::Header { title } => ItemKind::Header {
                    title: title.clone(),
                },
                Item::Video {
                    file: name,
                    name: shown,
                } => ItemKind::Media {
                    blob: put(file(name)?)?,
                    name: shown.clone(),
                    looped: false,
                },
                Item::Deck { file: name, title } => {
                    let deck = Deck {
                        id: DeckId::from_uuid(derived(def.id, 0x1000 + n as u128)),
                        title: title.clone(),
                        source: DeckSource::File { name: name.clone() },
                        current: Version {
                            blob: put(file(name)?)?,
                            fetched_at: Timestamp(0),
                            source_version: None,
                        },
                        pending: None,
                    };
                    let id = deck.id;
                    set.decks.push(deck);
                    ItemKind::Deck { deck: id }
                }
                Item::Song { title } => ItemKind::Song {
                    song: set
                        .songs
                        .iter()
                        .find(|s: &&Song| &s.title == title)
                        .ok_or_else(|| format!("no training song {title}"))?
                        .id,
                    arrangement: None,
                },
                Item::Scripture {
                    reference,
                    translation,
                } => ItemKind::Scripture {
                    passage: parse_reference(reference)
                        .map_err(|e| format!("{reference}: {e:?}"))?,
                    translation: BUNDLED
                        .iter()
                        .find(|b| b.abbreviation == translation)
                        .ok_or_else(|| format!("no bundled Bible {translation}"))?
                        .id,
                },
                Item::Text { title, body } => ItemKind::Text {
                    title: title.clone(),
                    body: body.clone(),
                    attribution: None,
                },
            };
            service.insert(n, kind);
            notes.push(entry.notes.clone());
        }
        // Fixed item IDs, in running order.
        let ordered: Vec<ItemId> = service.ordered().into_iter().map(|(id, _)| id).collect();
        let mut items = BTreeMap::new();
        for (n, old) in ordered.into_iter().enumerate() {
            let mut item = service.items.remove(&old).expect("listed above");
            item.notes = std::mem::take(&mut notes[n]);
            items.insert(ItemId::from_uuid(derived(def.id, n as u128 + 1)), item);
        }
        service.items = items;
        set.services.push(service);
    }
    Ok(set)
}

/// Builds the training services and gives them to the library. Run at
/// every start; a failure leaves the app without training, never stopped.
pub fn install(library: &Library) -> Result<(), String> {
    library.install_training(build(library)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_file_is_used_and_every_name_exists() {
        let mut used = Vec::new();
        for def in definitions().unwrap() {
            for entry in def.items {
                match entry.item {
                    Item::Video { file, name } => {
                        assert!(name.ends_with(".mp4"), "{name} plays as a video");
                        used.push(file);
                    }
                    Item::Deck { file, .. } => used.push(file),
                    _ => {}
                }
            }
        }
        for (name, bytes) in FILES {
            assert!(
                used.iter().any(|u| u == name),
                "{name} is bundled but unused"
            );
            assert!(!bytes.is_empty(), "{name} is empty");
        }
        for name in &used {
            file(name).unwrap();
        }
    }

    #[test]
    fn ids_are_the_same_at_every_start() {
        let dir = tempfile::tempdir().unwrap();
        let key = lampstand_sync::team::DeviceKeypair::generate();
        let library = Library::open(dir.path(), key).unwrap();
        let a = build(&library).unwrap();
        let b = build(&library).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.services.len(), 3);
        let titles: Vec<&str> = a.services.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Training 0: Orientation",
                "Training 1: Run a service",
                "Training 2: Confident operator"
            ]
        );
    }
}
