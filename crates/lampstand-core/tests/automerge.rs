//! The library survives a trip through Automerge, and edits made on two
//! devices merge (CD-1).

use automerge::{ActorId, AutoCommit};
use autosurgeon::{hydrate, reconcile};
use lampstand_core::library::Library;
use lampstand_core::safe_area::SafeArea;
use lampstand_core::service::{ItemKind, Service, ServiceItem};
use lampstand_core::song::{Section, Song};

fn item(id: &str, kind: ItemKind) -> ServiceItem {
    ServiceItem {
        id: id.into(),
        kind,
    }
}

fn library() -> Library {
    let mut lib = Library::default();
    lib.songs.insert(
        "amazing-grace".into(),
        Song {
            title: "Amazing Grace".into(),
            author: Some("John Newton".into()),
            copyright: Some("Public domain".into()),
            ccli_number: Some(22025),
            key: Some("G".into()),
            tempo: Some(72),
            sections: vec![
                Section {
                    label: "V1".into(),
                    lines: vec!["Amazing grace, how sweet the sound".into()],
                },
                Section {
                    label: "V2".into(),
                    lines: vec!["'Twas grace that taught my heart to fear".into()],
                },
            ],
            arrangement: vec!["V1".into(), "V2".into(), "V1".into()],
        },
    );
    lib.services.insert(
        "2026-10-25".into(),
        Service {
            title: "Sunday morning".into(),
            date: Some("2026-10-25".into()),
            items: vec![
                item(
                    "a",
                    ItemKind::Song {
                        song_id: "amazing-grace".into(),
                    },
                ),
                item(
                    "b",
                    ItemKind::Scripture {
                        reference: "Jn 3:16-18".into(),
                    },
                ),
                item(
                    "c",
                    ItemKind::Deck {
                        deck_id: "sermon".into(),
                    },
                ),
            ],
        },
    );
    lib
}

fn save(lib: &Library) -> AutoCommit {
    let mut doc = AutoCommit::new();
    reconcile(&mut doc, lib).unwrap();
    doc
}

#[test]
fn library_round_trips_through_a_document() {
    let lib = library();
    let loaded: Library = hydrate(&save(&lib)).unwrap();
    assert_eq!(loaded, lib);
}

#[test]
fn library_round_trips_through_saved_bytes() {
    let lib = library();
    let bytes = save(&lib).save();
    let loaded: Library = hydrate(&AutoCommit::load(&bytes).unwrap()).unwrap();
    assert_eq!(loaded, lib);
}

#[test]
fn edits_on_two_devices_merge() {
    let lib = library();
    let mut office = save(&lib);
    let mut laptop = office.fork().with_actor(ActorId::random());

    // The office adds a song and moves the sermon to the top of the order.
    let mut office_lib = lib.clone();
    office_lib.songs.insert(
        "new-song".into(),
        Song {
            title: "New song".into(),
            ..Default::default()
        },
    );
    let service = office_lib.services.get_mut("2026-10-25").unwrap();
    let sermon = service.items.pop().unwrap();
    service.items.insert(0, sermon);
    reconcile(&mut office, &office_lib).unwrap();

    // Meanwhile the laptop changes the key and adds an announcement deck.
    let mut laptop_lib = lib.clone();
    laptop_lib.songs.get_mut("amazing-grace").unwrap().key = Some("A".into());
    let service = laptop_lib.services.get_mut("2026-10-25").unwrap();
    service.items.push(item(
        "d",
        ItemKind::Deck {
            deck_id: "announcements".into(),
        },
    ));
    reconcile(&mut laptop, &laptop_lib).unwrap();

    office.merge(&mut laptop).unwrap();
    let merged: Library = hydrate(&office).unwrap();

    assert!(merged.songs.contains_key("new-song"));
    assert_eq!(merged.songs["amazing-grace"].key.as_deref(), Some("A"));
    let order: Vec<&str> = merged.services["2026-10-25"]
        .items
        .iter()
        .map(|i| i.id.as_str())
        .collect();
    assert_eq!(order, ["c", "a", "b", "d"]);
}

#[test]
fn service_item_json_is_tagged() {
    let json = serde_json::to_value(item(
        "b",
        ItemKind::Scripture {
            reference: "Jn 3:16".into(),
        },
    ))
    .unwrap();
    assert_eq!(
        json,
        serde_json::json!({"id": "b", "kind": {"type": "scripture", "reference": "Jn 3:16"}})
    );
}

#[test]
fn library_round_trips_through_json() {
    let lib = library();
    let json = serde_json::to_string(&lib).unwrap();
    assert_eq!(serde_json::from_str::<Library>(&json).unwrap(), lib);
}

#[test]
fn safe_area_round_trips_through_a_document() {
    let safe = SafeArea {
        top: 0.05,
        right: 0.025,
        bottom: 0.25,
        left: 0.025,
    };
    let mut doc = AutoCommit::new();
    reconcile(&mut doc, safe).unwrap();
    assert_eq!(hydrate::<_, SafeArea>(&doc).unwrap(), safe);
}
