//! T16 acceptance tests: documents round-trip, in-memory repos converge,
//! blob writes are atomic and verified.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use lampstand_core::deck::{Deck, DeckSource, Version};
use lampstand_core::geom::{Color, Point, Rect};
use lampstand_core::id::{BlobHash, Timestamp};
use lampstand_core::scene::{
    Direction, HAlign, Outline, Shadow, Transition, TransitionKind, VAlign,
};
use lampstand_core::scripture::{Book, ChapterVerse, Licence, Passage, Translation, TranslationId};
use lampstand_core::service::{
    CountdownTarget, ItemId, ItemKind, Position, Service, ServiceItem, Sound, SoundStart,
};
use lampstand_core::song::{Section, Song};
use lampstand_core::theme::{Background, TextStyle, Theme, ThemeId};
use lampstand_sync::team::DeviceKeypair;
use lampstand_sync::{actor_device, Doc, DocChanged, DocKind, Repo, SyncError};
use uuid::Uuid;

fn open(dir: &Path) -> Repo {
    Repo::open(dir.to_path_buf(), DeviceKeypair::generate()).unwrap()
}

/// Passes sync messages both ways for every document either side holds,
/// until neither has anything left to say.
fn sync(a: &Repo, b: &Repo) {
    for _ in 0..20 {
        let mut ids: Vec<Uuid> = a.documents().into_iter().map(|(_, id)| id).collect();
        ids.extend(b.documents().into_iter().map(|(_, id)| id));
        ids.sort();
        ids.dedup();
        let mut quiet = true;
        for id in ids {
            if let Some(m) = a.generate_sync_message(b.device(), id) {
                b.receive_sync_message(a.device(), m).unwrap();
                quiet = false;
            }
            if let Some(m) = b.generate_sync_message(a.device(), id) {
                a.receive_sync_message(b.device(), m).unwrap();
                quiet = false;
            }
        }
        if quiet {
            return;
        }
    }
    panic!("repos did not converge in 20 rounds");
}

/// `Position`'s field is private and `Position::between` is T02's, so
/// build one the way a synced document would.
fn pos(s: &str) -> Position {
    let mut doc = automerge::AutoCommit::new();
    autosurgeon::reconcile_prop(&mut doc, automerge::ROOT, "p", s).unwrap();
    autosurgeon::hydrate_prop(&doc, automerge::ROOT, "p").unwrap()
}

fn header(position: &str, title: &str) -> ServiceItem {
    ServiceItem {
        position: pos(position),
        kind: ItemKind::Header {
            title: title.into(),
        },
        notes: String::new(),
        obs_scene: None,
        loop_seconds: None,
        backing: None,
    }
}

fn hymn() -> Song {
    Song {
        author: Some("John Newton".into()),
        copyright: Some("Public Domain".into()),
        ccli_number: Some(22025),
        key: Some("G".into()),
        tempo: Some(80),
        sections: vec![
            Section {
                label: "V1".into(),
                lines: vec![
                    "[G]Amazing [G7]grace".into(),
                    "how [C]sweet the sound".into(),
                ],
            },
            Section {
                label: "C".into(),
                lines: vec!["My chains are gone".into()],
            },
        ],
        arrangement: vec!["V1".into(), "C".into(), "V1".into()],
        ..Song::new("Amazing Grace")
    }
}

fn theme() -> Theme {
    let white = Color {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };
    Theme {
        id: ThemeId::generate(),
        name: "Lower third".into(),
        text: TextStyle {
            font_family: "Noto Sans".into(),
            size: 64.0,
            min_size: 40.5,
            weight: 700,
            italic: false,
            color: white,
            align: HAlign::Center,
            valign: VAlign::Bottom,
            line_spacing: 1.2,
            uppercase: true,
            shadow: Some(Shadow {
                color: Color { a: 128, ..white },
                offset: Point { x: 2.0, y: 3.0 },
                blur: 4.0,
            }),
            outline: Some(Outline {
                color: white,
                width: 1.5,
            }),
        },
        background: Background::Image(BlobHash::of(b"sunrise.jpg")),
        text_area: Rect {
            x: 0.0,
            y: 0.7,
            width: 1.0,
            height: 0.3,
        },
        max_lines: 2,
        transition: Transition {
            kind: TransitionKind::Push(Direction::Left),
            duration_ms: 400,
        },
        show_copyright: true,
    }
}

fn sound(name: &str, length_ms: Option<u32>, start: SoundStart) -> Sound {
    Sound {
        name: name.into(),
        file: BlobHash::of(name.as_bytes()),
        length_ms,
        start,
        keep_off_stream: false,
    }
}

fn every_item_kind() -> Service {
    let mut s = Service::new("Sunday 10am");
    s.starts_at = Some(Timestamp(1_790_000_000_000));
    s.time_zone = Some("Europe/London".into());
    s.theme = Some(ThemeId::generate());
    let blob = BlobHash::of(b"video");
    let kinds = [
        ItemKind::Header {
            title: "Worship".into(),
        },
        ItemKind::Song {
            song: Song::new("x").id,
            arrangement: Some(vec!["V1".into(), "C".into()]),
        },
        ItemKind::Song {
            song: Song::new("y").id,
            arrangement: None,
        },
        ItemKind::Scripture {
            passage: Passage {
                book: Book::new(43).unwrap(),
                start: ChapterVerse {
                    chapter: 3,
                    verse: 16,
                },
                end: ChapterVerse {
                    chapter: 3,
                    verse: ChapterVerse::END_OF_CHAPTER,
                },
            },
            translation: TranslationId::generate(),
        },
        ItemKind::Deck {
            deck: lampstand_core::deck::DeckId::generate(),
        },
        ItemKind::Media {
            blob,
            name: "intro.mp4".into(),
            looped: true,
        },
        ItemKind::Countdown {
            target: CountdownTarget::At(Timestamp(1_790_000_000_000)),
            label: "Starting soon".into(),
        },
        ItemKind::Countdown {
            target: CountdownTarget::Seconds(300),
            label: String::new(),
        },
        ItemKind::Text {
            title: "Quote".into(),
            body: "Line one\nline two\n\n---\nSecond slide".into(),
            attribution: Some("C. S. Lewis".into()),
        },
        ItemKind::Text {
            title: String::new(),
            body: String::new(),
            attribution: None,
        },
        ItemKind::Announcements {
            slides: vec![blob, BlobHash::of(b"other")],
            seconds_each: 8,
        },
        ItemKind::Sound {
            sound: sound("Walk-in music", Some(185_000), SoundStart::OnItsOwn),
        },
        ItemKind::Sound {
            sound: Sound {
                keep_off_stream: true,
                ..sound("Cue", None, SoundStart::Waits)
            },
        },
    ];
    for (i, kind) in kinds.into_iter().enumerate() {
        let item = ServiceItem {
            position: pos(&format!("a{i}")),
            kind,
            notes: format!("note {i}"),
            obs_scene: (i % 2 == 0).then(|| format!("Scene {i}")),
            loop_seconds: None,
            // The first song carries a backing track.
            backing: (i == 1).then(|| sound("Track", Some(240_000), SoundStart::Waits)),
        };
        s.items.insert(ItemId::generate(), item);
    }
    s
}

fn deck() -> Deck {
    let version = |n: &[u8]| Version {
        blob: BlobHash::of(n),
        fetched_at: Timestamp(42),
        source_version: Some("17".into()),
    };
    Deck {
        id: lampstand_core::deck::DeckId::generate(),
        title: "Sermon".into(),
        source: DeckSource::GoogleDrive {
            file_id: "1AbC".into(),
            google_slides: true,
        },
        current: version(b"v1"),
        pending: Some(version(b"v2")),
    }
}

fn translation() -> Translation {
    Translation {
        id: TranslationId::generate(),
        name: "World English Bible".into(),
        abbreviation: "WEB".into(),
        language: "en".into(),
        licence: Licence::Licensed {
            notice: Some("Used by permission".into()),
        },
        text: BlobHash::of(b"web"),
    }
}

fn round_trip<T: Doc + PartialEq + std::fmt::Debug>(repo: &Repo, doc: T) {
    repo.put(&doc).unwrap();
    assert_eq!(repo.get::<T>(doc.doc_id()).unwrap().as_ref(), Some(&doc));
}

#[test]
fn every_document_type_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let repo = open(dir.path());
    round_trip(&repo, hymn());
    round_trip(&repo, every_item_kind());
    round_trip(&repo, theme());
    round_trip(&repo, deck());
    round_trip(&repo, translation());
    let mut plain = theme();
    plain.background = Background::None;
    plain.transition = Transition::CUT;
    plain.text.shadow = None;
    round_trip(&repo, plain);
}

/// A service written before services had a time zone has no `time_zone`
/// key in its document; it reads as `None`, not an error.
#[test]
fn a_service_saved_before_time_zones_reads_without_one() {
    use automerge::transaction::Transactable;
    let mut service = Service::new("Old");
    service.starts_at = Some(Timestamp(1_790_000_000_000));
    let mut doc = automerge::AutoCommit::new();
    autosurgeon::reconcile(&mut doc, &service).unwrap();
    doc.delete(automerge::ROOT, "time_zone").unwrap();
    let read: Service = autosurgeon::hydrate(&doc).unwrap();
    assert_eq!(read, service);
}

/// An item written before items could name an OBS scene has no
/// `obs_scene` key; it reads as `None`.
#[test]
fn an_item_saved_before_obs_scenes_reads_without_one() {
    use automerge::transaction::Transactable;
    use automerge::ReadDoc;
    let mut service = Service::new("Old");
    let id = service.insert(
        0,
        ItemKind::Header {
            title: "Welcome".into(),
        },
    );
    let mut doc = automerge::AutoCommit::new();
    autosurgeon::reconcile(&mut doc, &service).unwrap();
    let (_, items) = doc.get(automerge::ROOT, "items").unwrap().unwrap();
    let (_, item) = doc.get(&items, id.to_string()).unwrap().unwrap();
    doc.delete(&item, "obs_scene").unwrap();
    let read: Service = autosurgeon::hydrate(&doc).unwrap();
    assert_eq!(read, service);
    assert_eq!(read.items[&id].obs_scene, None);
}

/// What Lampstand 0.7.0 and the 0.8.0 builds before sound read and write: the
/// service as it was, with no `backing` on an item and no `Sound` kind. Used
/// to say what an older version does with a document that has sound in it.
mod before_sound {
    use std::collections::BTreeMap;

    use lampstand_core::id::{BlobHash, Timestamp};
    use lampstand_core::service::{ItemId, Position, ServiceId};
    use lampstand_core::song::SongId;
    use lampstand_core::theme::ThemeId;

    #[derive(Debug, Clone, PartialEq, autosurgeon::Hydrate, autosurgeon::Reconcile)]
    pub struct Service {
        pub id: ServiceId,
        pub title: String,
        pub starts_at: Option<Timestamp>,
        #[autosurgeon(missing = "Option::default")]
        pub time_zone: Option<String>,
        pub theme: Option<ThemeId>,
        #[autosurgeon(with = "autosurgeon::map_with_parseable_keys")]
        pub items: BTreeMap<ItemId, Item>,
    }

    #[derive(Debug, Clone, PartialEq, autosurgeon::Hydrate, autosurgeon::Reconcile)]
    pub struct Item {
        pub position: Position,
        pub kind: Kind,
        pub notes: String,
        #[autosurgeon(missing = "Option::default")]
        pub obs_scene: Option<String>,
        #[autosurgeon(missing = "Option::default")]
        pub loop_seconds: Option<u16>,
    }

    #[derive(Debug, Clone, PartialEq, autosurgeon::Hydrate, autosurgeon::Reconcile)]
    pub enum Kind {
        Header {
            title: String,
        },
        Song {
            song: SongId,
            arrangement: Option<Vec<String>>,
        },
        Media {
            blob: BlobHash,
            name: String,
            looped: bool,
        },
    }
}

/// A service with no sound in it is the document it was: the item map has
/// no `backing` key, putting it again after a read writes nothing, and a
/// document written before sound existed reads back and re-saves unchanged.
#[test]
fn a_service_without_sound_is_the_document_it_was() {
    use automerge::transaction::Transactable;
    use automerge::ReadDoc;
    let mut service = Service::new("Old");
    let header = service.insert(
        0,
        ItemKind::Header {
            title: "Welcome".into(),
        },
    );
    service.insert(
        1,
        ItemKind::Song {
            song: Song::new("x").id,
            arrangement: None,
        },
    );
    let mut doc = automerge::AutoCommit::new();
    autosurgeon::reconcile(&mut doc, &service).unwrap();
    let (_, items) = doc.get(automerge::ROOT, "items").unwrap().unwrap();
    for key in doc.keys(&items).collect::<Vec<String>>() {
        let (_, item) = doc.get(&items, key).unwrap().unwrap();
        let keys: Vec<String> = doc.keys(&item).collect();
        assert!(!keys.contains(&"backing".to_string()), "{keys:?}");
    }

    // The same document with the key an older writer never had: reading and
    // reconciling it again adds no operation.
    let (_, item) = doc.get(&items, header.to_string()).unwrap().unwrap();
    let _ = doc.delete(&item, "backing");
    let heads = doc.get_heads();
    let read: Service = autosurgeon::hydrate(&doc).unwrap();
    assert_eq!(read, service);
    autosurgeon::reconcile(&mut doc, &read).unwrap();
    assert_eq!(
        doc.get_heads(),
        heads,
        "re-saving an unchanged service wrote something"
    );
}

/// What an older version does with sound. A backing track on a song is an
/// extra key the old reader does not know: it reads the song as before, can
/// edit it, and the track is still there afterwards. A music-only item is a
/// kind the old reader does not know: it cannot read that service (an
/// error, not a wrong answer), and it cannot overwrite what it cannot read.
#[test]
fn an_older_version_loses_no_sound() {
    let mut service = Service::new("Sunday");
    let song = service.insert(
        0,
        ItemKind::Song {
            song: Song::new("x").id,
            arrangement: None,
        },
    );
    service.items.get_mut(&song).unwrap().backing =
        Some(sound("Track", Some(240_000), SoundStart::Waits));
    let mut doc = automerge::AutoCommit::new();
    autosurgeon::reconcile(&mut doc, &service).unwrap();

    // The old reader reads the song and edits its notes, as 0.7.0 would.
    let mut old: before_sound::Service = autosurgeon::hydrate(&doc).unwrap();
    old.items.get_mut(&song).unwrap().notes = "capo 2".into();
    autosurgeon::reconcile(&mut doc, &old).unwrap();

    // The current reader still has the track, and the edit.
    let back: Service = autosurgeon::hydrate(&doc).unwrap();
    assert_eq!(back.items[&song].notes, "capo 2");
    assert_eq!(
        back.items[&song].backing,
        Some(sound("Track", Some(240_000), SoundStart::Waits))
    );

    // With a music-only item in the order, the old reader fails to read.
    service.insert(
        1,
        ItemKind::Sound {
            sound: sound("Walk-in", Some(1000), SoundStart::OnItsOwn),
        },
    );
    autosurgeon::reconcile(&mut doc, &service).unwrap();
    assert!(autosurgeon::hydrate::<_, before_sound::Service>(&doc).is_err());
    // The data is untouched in the document for the current reader.
    let still: Service = autosurgeon::hydrate(&doc).unwrap();
    assert_eq!(still.items.len(), 2);
}

#[test]
fn sound_items_and_backing_tracks_sync_between_two_devices() {
    let (da, db) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (a, b) = (open(da.path()), open(db.path()));
    let mut service = Service::new("Sunday");
    service.insert(
        0,
        ItemKind::Sound {
            sound: sound("Walk-in music", Some(185_000), SoundStart::OnItsOwn),
        },
    );
    let song = service.insert(
        1,
        ItemKind::Song {
            song: Song::new("x").id,
            arrangement: None,
        },
    );
    service.items.get_mut(&song).unwrap().backing =
        Some(sound("Track", Some(240_000), SoundStart::Waits));
    a.put(&service).unwrap();
    sync(&a, &b);
    let on_b: Service = b.get(service.id.uuid()).unwrap().unwrap();
    assert_eq!(on_b, service);
    assert_eq!(on_b.sound_ms(), (185_000, true));

    // B changes how the track starts; A sees it, and the rest stays.
    let mut edited = on_b.clone();
    edited
        .items
        .get_mut(&song)
        .unwrap()
        .backing
        .as_mut()
        .unwrap()
        .start = SoundStart::OnItsOwn;
    edited
        .items
        .get_mut(&song)
        .unwrap()
        .backing
        .as_mut()
        .unwrap()
        .keep_off_stream = true;
    b.put(&edited).unwrap();
    sync(&a, &b);
    let on_a: Service = a.get(service.id.uuid()).unwrap().unwrap();
    assert_eq!(on_a, edited);

    // A takes the track off while B renames it: both ends converge.
    let mut off = on_a.clone();
    off.items.get_mut(&song).unwrap().backing = None;
    a.put(&off).unwrap();
    sync(&a, &b);
    let on_b: Service = b.get(service.id.uuid()).unwrap().unwrap();
    assert_eq!(on_b.items[&song].backing, None);
}

#[test]
fn put_get_list_delete() {
    let dir = tempfile::tempdir().unwrap();
    let repo = open(dir.path());
    let a = hymn();
    let b = Song::new("Be Thou My Vision");
    let s = Service::new("Sunday");
    repo.put(&a).unwrap();
    repo.put(&b).unwrap();
    repo.put(&s).unwrap();

    let mut songs = vec![a.id.uuid(), b.id.uuid()];
    songs.sort();
    assert_eq!(repo.list(DocKind::Song), songs);
    assert_eq!(repo.list(DocKind::Service), [s.id.uuid()]);
    assert!(repo.list(DocKind::Theme).is_empty());
    // The wrong type, or an unknown ID, is simply not there.
    assert_eq!(repo.get::<Service>(a.id.uuid()).unwrap(), None);
    assert_eq!(repo.get::<Song>(Uuid::now_v7()).unwrap(), None);

    assert!(!repo.is_deleted(DocKind::Song, a.id.uuid()).unwrap());
    repo.delete(DocKind::Song, a.id.uuid()).unwrap();
    assert_eq!(repo.list(DocKind::Song), [b.id.uuid()]);
    assert_eq!(repo.get::<Song>(a.id.uuid()).unwrap(), None);
    // Deleted is not the same as never existed, or another kind.
    assert!(repo.is_deleted(DocKind::Song, a.id.uuid()).unwrap());
    assert!(!repo.is_deleted(DocKind::Service, a.id.uuid()).unwrap());
    assert!(!repo.is_deleted(DocKind::Song, Uuid::now_v7()).unwrap());

    // Putting it again brings it back.
    repo.put(&a).unwrap();
    assert!(!repo.is_deleted(DocKind::Song, a.id.uuid()).unwrap());
    assert_eq!(repo.get::<Song>(a.id.uuid()).unwrap(), Some(a));
    assert_eq!(repo.list(DocKind::Song).len(), 2);
}

#[test]
fn documents_survive_reopening() {
    let dir = tempfile::tempdir().unwrap();
    let mut song = hymn();
    let gone = Song::new("Deleted");
    {
        let repo = open(dir.path());
        // Enough puts to compact the chunks at least once.
        for n in 0..40 {
            song.tempo = Some(60 + n);
            repo.put(&song).unwrap();
        }
        repo.put(&gone).unwrap();
        repo.delete(DocKind::Song, gone.id.uuid()).unwrap();
    }
    let chunks = fs::read_dir(dir.path().join("docs").join(song.id.to_string()))
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .path()
                .extension()
                .is_some_and(|x| x == "chunk")
        })
        .count();
    assert!(chunks <= 33, "{chunks} chunks");

    let repo = open(dir.path());
    assert_eq!(
        repo.get::<Song>(song.id.uuid()).unwrap(),
        Some(song.clone())
    );
    assert_eq!(repo.list(DocKind::Song), [song.id.uuid()]);
    assert_eq!(repo.get::<Song>(gone.id.uuid()).unwrap(), None);

    // Edits after reopening carry on from the stored history.
    song.title = "Amazing Grace (My Chains Are Gone)".into();
    repo.put(&song).unwrap();
    drop(repo);
    assert_eq!(
        open(dir.path()).get::<Song>(song.id.uuid()).unwrap(),
        Some(song)
    );
}

#[test]
fn unchanged_put_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let repo = open(dir.path());
    let song = hymn();
    repo.put(&song).unwrap();
    let events = repo.subscribe();
    let before = repo.generate_sync_message(repo.device(), song.id.uuid());
    repo.put(&song).unwrap();
    assert!(events.try_recv().is_err());
    // Same heads, so a fresh peer gets the same opening message.
    repo.forget_peer(repo.device());
    assert_eq!(
        repo.generate_sync_message(repo.device(), song.id.uuid()),
        before
    );
}

#[test]
fn subscribers_hear_local_and_remote_changes() {
    let (da, db) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (a, b) = (open(da.path()), open(db.path()));
    let a_events = a.subscribe();
    let b_events = b.subscribe();
    let song = hymn();
    a.put(&song).unwrap();
    assert_eq!(
        a_events.try_recv().unwrap(),
        DocChanged {
            kind: DocKind::Song,
            id: song.id.uuid(),
            remote: false
        }
    );
    sync(&a, &b);
    let heard: Vec<DocChanged> = b_events.try_iter().collect();
    assert!(heard.contains(&DocChanged {
        kind: DocKind::Song,
        id: song.id.uuid(),
        remote: true
    }));
    assert!(heard.iter().any(|e| e.kind == DocKind::Team && e.remote));
}

#[test]
fn concurrent_edits_to_different_sections_converge() {
    let (da, db) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (a, b) = (open(da.path()), open(db.path()));
    let song = hymn();
    a.put(&song).unwrap();
    sync(&a, &b);
    assert_eq!(b.get::<Song>(song.id.uuid()).unwrap(), Some(song.clone()));
    assert_eq!(b.list(DocKind::Song), [song.id.uuid()]);

    // Offline, A fixes verse 1 and B fixes the chorus.
    let mut on_a = song.clone();
    on_a.sections[0].lines[1] = "that saved a wretch like me".into();
    a.put(&on_a).unwrap();
    let mut on_b = b.get::<Song>(song.id.uuid()).unwrap().unwrap();
    on_b.sections[1].lines[0] = "My chains are gone, I've been set free".into();
    on_b.key = Some("A".into());
    b.put(&on_b).unwrap();

    sync(&a, &b);
    let merged_a = a.get::<Song>(song.id.uuid()).unwrap().unwrap();
    let merged_b = b.get::<Song>(song.id.uuid()).unwrap().unwrap();
    assert_eq!(merged_a, merged_b);
    assert_eq!(merged_a.sections[0].lines[1], "that saved a wretch like me");
    assert_eq!(
        merged_a.sections[1].lines[0],
        "My chains are gone, I've been set free"
    );
    assert_eq!(merged_a.key.as_deref(), Some("A"));
    assert_eq!(merged_a.sections.len(), 2);
}

#[test]
fn concurrent_moves_of_one_item_do_not_duplicate_it() {
    let (da, db) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (a, b) = (open(da.path()), open(db.path()));
    let mut service = Service::new("Sunday");
    let ids: Vec<ItemId> = (0..3).map(|_| ItemId::generate()).collect();
    for (id, (p, t)) in ids
        .iter()
        .zip([("b", "Welcome"), ("d", "Song"), ("f", "Sermon")])
    {
        service.items.insert(*id, header(p, t));
    }
    a.put(&service).unwrap();
    sync(&a, &b);

    // Both planners move the sermon, to different places.
    let sermon = ids[2];
    let mut on_a = service.clone();
    on_a.items.get_mut(&sermon).unwrap().position = pos("a");
    a.put(&on_a).unwrap();
    let mut on_b = b.get::<Service>(service.id.uuid()).unwrap().unwrap();
    on_b.items.get_mut(&sermon).unwrap().position = pos("c");
    b.put(&on_b).unwrap();

    sync(&a, &b);
    let merged = a.get::<Service>(service.id.uuid()).unwrap().unwrap();
    assert_eq!(
        Some(&merged),
        b.get::<Service>(service.id.uuid()).unwrap().as_ref()
    );
    let order: Vec<ItemId> = merged.ordered().into_iter().map(|(id, _)| id).collect();
    assert_eq!(order.len(), 3);
    assert_eq!(order.iter().filter(|id| **id == sermon).count(), 1);
    // One of the two moves won.
    let p = merged.items[&sermon].position.as_str().to_string();
    assert!(p == "a" || p == "c", "{p}");
}

#[test]
fn concurrent_item_add_and_remove_merge() {
    let (da, db) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (a, b) = (open(da.path()), open(db.path()));
    let mut service = Service::new("Sunday");
    let welcome = ItemId::generate();
    service.items.insert(welcome, header("b", "Welcome"));
    a.put(&service).unwrap();
    sync(&a, &b);

    let mut on_a = service.clone();
    let offering = ItemId::generate();
    on_a.items.insert(offering, header("c", "Offering"));
    a.put(&on_a).unwrap();
    let mut on_b = service.clone();
    on_b.items.remove(&welcome);
    b.put(&on_b).unwrap();

    sync(&a, &b);
    let merged = b.get::<Service>(service.id.uuid()).unwrap().unwrap();
    assert_eq!(merged.items.keys().copied().collect::<Vec<_>>(), [offering]);
}

#[test]
fn deletes_sync_and_index_merges_across_devices() {
    let (da, db) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (a, b) = (open(da.path()), open(db.path()));
    // Each device adds a song before they ever meet.
    let (sa, sb) = (Song::new("From A"), Song::new("From B"));
    a.put(&sa).unwrap();
    b.put(&sb).unwrap();
    sync(&a, &b);
    let mut both = vec![sa.id.uuid(), sb.id.uuid()];
    both.sort();
    assert_eq!(a.list(DocKind::Song), both);
    assert_eq!(b.list(DocKind::Song), both);

    b.delete(DocKind::Song, sa.id.uuid()).unwrap();
    sync(&a, &b);
    assert_eq!(a.list(DocKind::Song), [sb.id.uuid()]);
    assert_eq!(a.get::<Song>(sa.id.uuid()).unwrap(), None);
    assert!(a.is_deleted(DocKind::Song, sa.id.uuid()).unwrap());
}

#[test]
fn changes_carry_the_device_key() {
    let dir = tempfile::tempdir().unwrap();
    let device = DeviceKeypair::generate();
    let key = device.public();
    let repo = Repo::open(dir.path().to_path_buf(), device).unwrap();
    let song = hymn();
    repo.put(&song).unwrap();
    drop(repo);

    let mut doc = automerge::Automerge::new();
    for entry in fs::read_dir(dir.path().join("docs").join(song.id.to_string())).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|x| x == "chunk") {
            doc.load_incremental(&fs::read(path).unwrap()).unwrap();
        }
    }
    let authors: Vec<_> = doc
        .get_changes(&[])
        .iter()
        .map(|c| actor_device(c.actor_id()))
        .collect();
    assert!(!authors.is_empty());
    assert!(authors.iter().all(|a| *a == Some(key)));
}

#[test]
fn wrong_kind_in_a_sync_message_is_refused() {
    let (da, db) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (a, b) = (open(da.path()), open(db.path()));
    let song = hymn();
    a.put(&song).unwrap();
    b.put(&song).unwrap();
    let mut m = a.generate_sync_message(b.device(), song.id.uuid()).unwrap();
    m.kind = DocKind::Service;
    assert!(matches!(
        b.receive_sync_message(a.device(), m),
        Err(SyncError::Corrupt(_))
    ));
}

#[test]
fn blobs_are_content_addressed_and_verified() {
    let dir = tempfile::tempdir().unwrap();
    let repo = open(dir.path());
    let blobs = repo.blobs();
    let bytes = b"PK\x03\x04 a deck".to_vec();
    let hash = blobs.put(&bytes).unwrap();
    assert_eq!(hash, BlobHash::of(&bytes));
    let path = blobs.path(&hash).unwrap();
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(
        path.file_name().unwrap().to_string_lossy(),
        hash.to_string()
    );
    // Writing the same bytes again is a no-op.
    assert_eq!(blobs.put(&bytes).unwrap(), hash);

    let other = BlobHash::of(b"not here");
    assert_eq!(blobs.missing(&[hash, other]), [other]);
    assert_eq!(blobs.path(&other), None);

    // A blob from a peer that does not match its hash is refused and
    // leaves nothing behind, not even a temp file.
    let refused = blobs.put_verified(&other, &b"something else"[..]);
    assert!(matches!(refused, Err(SyncError::Corrupt(_))));
    assert_eq!(blobs.path(&other), None);
    let names: Vec<String> = fs::read_dir(dir.path().join("blobs"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, [hash.to_string()]);

    blobs.put_verified(&other, &b"not here"[..]).unwrap();
    assert!(blobs.missing(&[hash, other]).is_empty());

    let source = dir.path().join("video.mp4");
    let big: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    fs::write(&source, &big).unwrap();
    let imported = blobs.import(&source).unwrap();
    assert_eq!(imported, BlobHash::of(&big));
    assert_eq!(fs::read(blobs.path(&imported).unwrap()).unwrap(), big);
}

#[test]
fn leftover_temp_files_are_cleaned_on_open() {
    let dir = tempfile::tempdir().unwrap();
    drop(open(dir.path()));
    let stray = dir.path().join("blobs").join(".tmp-0123456789abcdef");
    fs::write(&stray, b"half a video").unwrap();
    drop(open(dir.path()));
    assert!(!stray.exists());
}

#[test]
fn item_map_keys_are_id_strings() {
    let mut doc = automerge::AutoCommit::new();
    let service = every_item_kind();
    autosurgeon::reconcile(&mut doc, &service).unwrap();
    use automerge::ReadDoc;
    let (_, items) = doc.get(automerge::ROOT, "items").unwrap().unwrap();
    let keys: Vec<String> = doc.keys(&items).collect();
    let expected: Vec<String> = service.items.keys().map(|k| k.to_string()).collect();
    assert_eq!(keys, expected);
    let (id, _) = doc.get(automerge::ROOT, "id").unwrap().unwrap();
    assert_eq!(id.into_string().unwrap(), service.id.to_string());
    let _: BTreeMap<ItemId, ServiceItem> = service.items;
}

/// Three devices edit one service at random while syncing random pairs
/// (the rest are partitioned), then all meet: they agree, and no item is
/// lost or duplicated.
#[test]
fn three_repos_converge_under_random_edits_and_partitions() {
    let dirs: Vec<_> = (0..3).map(|_| tempfile::tempdir().unwrap()).collect();
    let repos: Vec<Repo> = dirs.iter().map(|d| open(d.path())).collect();
    let mut service = Service::new("Sunday");
    let ids: Vec<ItemId> = (0..5).map(|_| ItemId::generate()).collect();
    for (i, id) in ids.iter().enumerate() {
        service
            .items
            .insert(*id, header(&format!("m{i}"), &format!("item {i}")));
    }
    repos[0].put(&service).unwrap();
    sync(&repos[0], &repos[1]);
    sync(&repos[1], &repos[2]);

    // xorshift, so failures replay.
    let mut seed = 0x9E37_79B9_7F4A_7C15_u64;
    let mut next = move |n: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % n as u64) as usize
    };
    let positions = ["0", "A", "M", "Z", "a", "m", "z"];
    for round in 0..60 {
        let who = next(3);
        let repo = &repos[who];
        let mut s = repo.get::<Service>(service.id.uuid()).unwrap().unwrap();
        let item = s.items.get_mut(&ids[next(ids.len())]).unwrap();
        if next(2) == 0 {
            item.position = pos(positions[next(positions.len())]);
        } else {
            item.notes = format!("round {round} on {who}");
        }
        repo.put(&s).unwrap();
        if next(3) == 0 {
            let (x, y) = (next(3), next(3));
            if x != y {
                sync(&repos[x], &repos[y]);
            }
        }
    }
    sync(&repos[0], &repos[1]);
    sync(&repos[1], &repos[2]);
    sync(&repos[0], &repos[1]);

    let all: Vec<Service> = repos
        .iter()
        .map(|r| r.get::<Service>(service.id.uuid()).unwrap().unwrap())
        .collect();
    assert_eq!(all[0], all[1]);
    assert_eq!(all[1], all[2]);
    let mut order: Vec<ItemId> = all[0].ordered().into_iter().map(|(id, _)| id).collect();
    order.sort();
    let mut expected = ids.clone();
    expected.sort();
    assert_eq!(order, expected);
}
