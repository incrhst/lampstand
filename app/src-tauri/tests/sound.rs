//! Sound in the order of the service (decision 0010): music-only items and a
//! song's backing track live in the service document, count in its timing,
//! sync with it, and their files follow by sync. Two apps stand in for two
//! computers, as in `church.rs`: their libraries exchange sync messages as
//! the real network does, and a fake link says who is connected and records
//! the files asked for.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use lampstand_api::{
    Event, ItemEstimate, LibraryRequest, Request, Response, ServiceRequest, SettingsRequest,
    SoundFileStatus, SoundSlot,
};
use lampstand_app::handler::{App, Placement, SyncLink, WindowHost};
use lampstand_core::id::{BlobHash, Timestamp};
use lampstand_core::output::OutputId;
use lampstand_core::service::{ItemId, ItemKind, Service, ServiceItem, Sound, SoundStart};
use lampstand_core::song::Song;
use lampstand_layout::Fonts;
use lampstand_render::output::DisplayInfo;
use lampstand_render::{OutputCommand, OutputSink};
use uuid::Uuid;

struct Sink;
impl OutputSink for Sink {
    fn send(&self, _: OutputId, _: OutputCommand) {}
}

struct Host;
impl WindowHost for Host {
    fn displays(&self) -> Vec<DisplayInfo> {
        Vec::new()
    }
    fn open(&mut self, _: OutputId, _: &str, _: Placement) -> Result<(), String> {
        Ok(())
    }
    fn close(&mut self, _: OutputId) {}
}

struct Link {
    connected: bool,
    fetched: Mutex<Vec<BlobHash>>,
}

impl SyncLink for Link {
    fn device_name(&self) -> String {
        "Test device".into()
    }
    fn device_key(&self) -> String {
        "a".into()
    }
    fn connected(&self) -> Vec<String> {
        if self.connected {
            vec!["peer".into()]
        } else {
            Vec::new()
        }
    }
    fn fetch(&self, hashes: Vec<BlobHash>) {
        self.fetched.lock().unwrap().extend(hashes);
    }
}

struct Device {
    app: App,
    link: Arc<Link>,
    events: Arc<Mutex<Vec<Event>>>,
    changes: std::sync::mpsc::Receiver<lampstand_sync::DocChanged>,
    _dir: tempfile::TempDir,
}

fn device(connected: bool) -> Device {
    let dir = tempfile::tempdir().unwrap();
    device_in(dir, connected)
}

fn device_in(dir: tempfile::TempDir, connected: bool) -> Device {
    let events = Arc::new(Mutex::new(Vec::new()));
    let recorded = events.clone();
    let mut app = App::open(
        dir.path(),
        Fonts::bundled_only(),
        Arc::new(Sink),
        Box::new(Host),
        Arc::new(move |e| recorded.lock().unwrap().push(e)),
    )
    .unwrap();
    let link = Arc::new(Link {
        connected,
        fetched: Mutex::new(Vec::new()),
    });
    app.set_sync_link(link.clone());
    let changes = app.library().subscribe();
    Device {
        app,
        link,
        events,
        changes,
        _dir: dir,
    }
}

impl Device {
    fn ok(&mut self, request: Request) -> Response {
        match self.app.handle(request) {
            Response::Error { message } => panic!("request failed: {message}"),
            r => r,
        }
    }

    fn service(&mut self, request: ServiceRequest) -> Service {
        match self.ok(Request::Service(request)) {
            Response::Service(s) => s,
            other => panic!("{other:?}"),
        }
    }

    fn create(&mut self) -> Service {
        self.service(ServiceRequest::Create {
            title: "Sunday".into(),
            starts_at: None,
            time_zone: None,
        })
    }

    fn insert(&mut self, service: &Service, kind: ItemKind) -> Service {
        self.service(ServiceRequest::InsertItem {
            service: service.id,
            index: usize::MAX,
            kind,
        })
    }

    fn estimates(&mut self, service: &Service) -> Vec<ItemEstimate> {
        match self.ok(Request::Service(ServiceRequest::Estimate(service.id))) {
            Response::Estimate(e) => e,
            other => panic!("{other:?}"),
        }
    }

    fn status(&mut self, service: &Service) -> Vec<SoundFileStatus> {
        match self.ok(Request::Service(ServiceRequest::SoundStatus(service.id))) {
            Response::SoundStatus(s) => s,
            other => panic!("{other:?}"),
        }
    }

    /// What the app thread does: hands library changes to the app.
    fn pump(&mut self) {
        let batch: Vec<_> = self.changes.try_iter().collect();
        if !batch.is_empty() {
            self.app.library_changed(&batch);
        }
    }

    fn heard_sound(&self) -> usize {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| matches!(e, Event::Changed { kind, .. } if kind == "sound"))
            .count()
    }
}

fn exchange(a: &App, b: &App) {
    let (a, b) = (a.library().repo(), b.library().repo());
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
    panic!("did not converge");
}

fn sound(name: &str, bytes: &[u8], ms: Option<u32>, start: SoundStart) -> Sound {
    Sound {
        name: name.into(),
        file: BlobHash::of(bytes),
        length_ms: ms,
        start,
        keep_off_stream: false,
    }
}

/// A WAV file of `seconds` of silence: 8 kHz, 8-bit, mono.
fn wav(seconds: u32) -> Vec<u8> {
    let data = 8000 * seconds;
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&8000u32.to_le_bytes());
    out.extend_from_slice(&8000u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&8u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    out.extend(std::iter::repeat_n(128u8, data as usize));
    out
}

fn item_of(s: &Service, f: impl Fn(&ServiceItem) -> bool) -> (ItemId, &ServiceItem) {
    s.items
        .iter()
        .map(|(id, item)| (*id, item))
        .find(|(_, item)| f(item))
        .expect("no such item")
}

#[test]
fn adding_a_sound_file_reads_its_name_and_length_and_refuses_what_is_not_sound() {
    let mut d = device(false);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Walk-in music.wav");
    std::fs::write(&path, wav(2)).unwrap();
    let file = match d.ok(Request::Library(LibraryRequest::AddSoundFile {
        path: path.to_str().unwrap().into(),
    })) {
        Response::SoundFile(f) => f,
        other => panic!("{other:?}"),
    };
    assert_eq!(file.name, "Walk-in music");
    assert_eq!(file.length_ms, Some(2000));
    assert!(d.app.library().repo().blobs().path(&file.file).is_some());

    // A text file is not sound: refused, and nothing is stored for it.
    let text = dir.path().join("notes.txt");
    std::fs::write(&text, b"not music").unwrap();
    match d.app.handle(Request::Library(LibraryRequest::AddSoundFile {
        path: text.to_str().unwrap().into(),
    })) {
        Response::Error { message } => {
            assert_eq!(
                message,
                "notes.txt does not have sound that Lampstand can read"
            )
        }
        other => panic!("{other:?}"),
    }
    assert!(d
        .app
        .library()
        .repo()
        .blobs()
        .path(&BlobHash::of(b"not music"))
        .is_none());
}

#[test]
fn a_music_only_item_and_a_backing_track_are_planned_and_edited() {
    let mut d = device(false);
    let s = d.create();
    let s = d.insert(
        &s,
        ItemKind::Sound {
            sound: sound("Walk-in", b"walk", Some(185_000), SoundStart::OnItsOwn),
        },
    );
    let s = d.insert(
        &s,
        ItemKind::Song {
            song: Song::new("x").id,
            arrangement: None,
        },
    );
    let (music, _) = item_of(&s, |i| matches!(i.kind, ItemKind::Sound { .. }));
    let (song, _) = item_of(&s, |i| matches!(i.kind, ItemKind::Song { .. }));

    // The track goes on the song; the name is trimmed and may not be blank.
    let track = sound("  Track  ", b"track", Some(240_000), SoundStart::Waits);
    let s = d.service(ServiceRequest::SetSound {
        service: s.id,
        item: song,
        slot: SoundSlot::Backing,
        sound: Some(track.clone()),
    });
    assert_eq!(s.items[&song].backing.as_ref().unwrap().name, "Track");
    let blank = Sound {
        name: "  ".into(),
        ..track.clone()
    };
    assert!(matches!(
        d.app.handle(Request::Service(ServiceRequest::SetSound {
            service: s.id,
            item: song,
            slot: SoundSlot::Backing,
            sound: Some(blank),
        })),
        Response::Error { message } if message == "a sound needs a name"
    ));

    // Changing how the music-only item starts, and keeping it off the stream.
    let s = d.service(ServiceRequest::SetSound {
        service: s.id,
        item: music,
        slot: SoundSlot::Item,
        sound: Some(Sound {
            start: SoundStart::Waits,
            keep_off_stream: true,
            ..sound("Walk-in", b"walk", Some(185_000), SoundStart::Waits)
        }),
    });
    let ItemKind::Sound { sound: now } = &s.items[&music].kind else {
        panic!()
    };
    assert_eq!(now.start, SoundStart::Waits);
    assert!(now.keep_off_stream);

    // The wrong place is refused in words.
    for (item, slot, why) in [
        (music, SoundSlot::Backing, "only a song has a backing track"),
        (song, SoundSlot::Item, "that item is not a sound"),
    ] {
        match d.app.handle(Request::Service(ServiceRequest::SetSound {
            service: s.id,
            item,
            slot,
            sound: Some(track.clone()),
        })) {
            Response::Error { message } => assert_eq!(message, why),
            other => panic!("{other:?}"),
        }
    }
    assert!(matches!(
        d.app.handle(Request::Service(ServiceRequest::SetSound {
            service: s.id,
            item: music,
            slot: SoundSlot::Item,
            sound: None,
        })),
        Response::Error { message } if message == "a sound item needs its sound"
    ));

    // Taking the track off leaves the song as it was.
    let s = d.service(ServiceRequest::SetSound {
        service: s.id,
        item: song,
        slot: SoundSlot::Backing,
        sound: None,
    });
    assert_eq!(s.items[&song].backing, None);
}

#[test]
fn music_only_items_count_in_the_timing_and_a_service_without_sound_is_unchanged() {
    let mut d = device(false);
    let plain = d.create();
    let plain = d.insert(
        &plain,
        ItemKind::Header {
            title: "Welcome".into(),
        },
    );
    // No sound: the estimate carries nothing for it, and the document has no
    // sound in it.
    assert!(d.estimates(&plain).is_empty());
    assert!(!plain.has_sound());
    assert!(d.status(&plain).is_empty());

    let s = d.create();
    let s = d.insert(
        &s,
        ItemKind::Sound {
            sound: sound("Walk-in", b"walk", Some(185_000), SoundStart::OnItsOwn),
        },
    );
    let s = d.insert(
        &s,
        ItemKind::Sound {
            sound: sound("Unknown", b"unknown", None, SoundStart::Waits),
        },
    );
    let est = d.estimates(&s);
    assert_eq!(est.len(), 2);
    let by_name = |name: &str| {
        let (id, _) = item_of(
            &s,
            |i| matches!(&i.kind, ItemKind::Sound { sound } if sound.name == name),
        );
        est.iter().find(|e| e.item == id).unwrap().clone()
    };
    let walk = by_name("Walk-in");
    assert_eq!(walk.sound_ms, Some(185_000));
    assert!(walk.sound && !walk.open_ended && walk.stills == 0 && walk.video_ms.is_none());
    let unknown = by_name("Unknown");
    assert_eq!(unknown.sound_ms, None);
    assert!(
        unknown.sound && !unknown.open_ended,
        "sound is not part of a loop"
    );
    assert_eq!(s.sound_ms(), (185_000, false));
}

#[test]
fn a_service_with_sound_opens_for_live_and_sound_shows_no_picture() {
    let mut d = device(false);
    let s = d.create();
    let s = d.insert(
        &s,
        ItemKind::Sound {
            sound: sound("Walk-in", b"walk", Some(1000), SoundStart::OnItsOwn),
        },
    );
    let loaded = match d.ok(Request::Service(ServiceRequest::Open(s.id))) {
        Response::Loaded(l) => l,
        other => panic!("{other:?}"),
    };
    assert_eq!(loaded.items.len(), 1);
    assert_eq!(loaded.items[0].title, "Walk-in");
    assert!(loaded.items[0].slides.is_empty(), "sound has no picture");
}

#[test]
fn plan_says_which_sound_files_are_not_on_this_device() {
    let mut d = device(false);
    let s = d.create();
    let present = d.app.library().repo().blobs().put(b"here").unwrap();
    let s = d.insert(
        &s,
        ItemKind::Sound {
            sound: Sound {
                file: present,
                ..sound("Here", b"here", Some(1000), SoundStart::Waits)
            },
        },
    );
    let s = d.insert(
        &s,
        ItemKind::Song {
            song: Song::new("x").id,
            arrangement: None,
        },
    );
    let (song, _) = item_of(&s, |i| matches!(i.kind, ItemKind::Song { .. }));
    let s = d.service(ServiceRequest::SetSound {
        service: s.id,
        item: song,
        slot: SoundSlot::Backing,
        sound: Some(sound("Track", b"not here", Some(1000), SoundStart::Waits)),
    });
    let (music, _) = item_of(&s, |i| matches!(i.kind, ItemKind::Sound { .. }));
    let status = d.status(&s);
    let of = |item: ItemId, backing: bool| {
        status
            .iter()
            .find(|f| f.item == item && f.backing == backing)
            .unwrap()
            .on_device
    };
    assert!(of(music, false));
    assert!(!of(song, true), "the backing track's file is missing here");

    // The file arrives: the same question now says it is here.
    d.app.library().repo().blobs().put(b"not here").unwrap();
    assert!(d.status(&s).iter().all(|f| f.on_device));
}

/// The moment the fetch tests run at (the real clock, which Plan's status
/// also reads), and an hour.
fn real_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}
const HOUR: i64 = 3_600_000;

/// A service starting at `starts` with one music-only item whose file is
/// `bytes`, on `d`.
fn dated_with_sound(d: &mut Device, starts: Option<i64>, bytes: &[u8]) -> Service {
    let s = d.service(ServiceRequest::Create {
        title: "Sunday".into(),
        starts_at: starts.map(Timestamp),
        time_zone: None,
    });
    d.insert(
        &s,
        ItemKind::Sound {
            sound: sound("Walk-in", bytes, Some(1000), SoundStart::Waits),
        },
    )
}

fn asked(d: &Device) -> Vec<BlobHash> {
    d.link.fetched.lock().unwrap().clone()
}

/// Sound is in the service document, so a second device gets the items with
/// the service. Its files do not travel inside documents: the device asks
/// the connected ones for what it lacks, a few at a time, again after a
/// growing wait, and tells Plan when they come.
#[test]
fn sound_syncs_with_the_service_and_its_files_are_asked_for_ahead_of_the_service() {
    let now_ms = real_now();
    let mut a = device(true);
    let mut b = device(true);
    let s = dated_with_sound(&mut a, Some(now_ms + HOUR), b"walk-in music");
    let s = a.insert(
        &s,
        ItemKind::Song {
            song: Song::new("x").id,
            arrangement: None,
        },
    );
    let (song, _) = item_of(&s, |i| matches!(i.kind, ItemKind::Song { .. }));
    let s = a.service(ServiceRequest::SetSound {
        service: s.id,
        item: song,
        slot: SoundSlot::Backing,
        sound: Some(sound(
            "Track",
            b"the track",
            Some(240_000),
            SoundStart::Waits,
        )),
    });

    exchange(&a.app, &b.app);
    b.pump();
    let on_b = match b.ok(Request::Service(ServiceRequest::Get(s.id))) {
        Response::Service(s) => s,
        other => panic!("{other:?}"),
    };
    assert_eq!(on_b, s, "the items arrive with the service");
    assert_eq!(on_b.sound_ms(), (1000, true));

    // B has neither file: it asks for both, once.
    let t0 = Instant::now();
    let now = Timestamp(now_ms);
    b.app.sound_tick_at(now, t0);
    let mut got = asked(&b);
    got.sort();
    let mut want = vec![BlobHash::of(b"walk-in music"), BlobHash::of(b"the track")];
    want.sort();
    assert_eq!(got, want);
    // Plan: both are missing and on their way.
    assert!(b.status(&s).iter().all(|f| !f.on_device && f.coming));

    // Inside the first wait nothing is asked again; after it, it is, and the
    // wait before the next ask doubles.
    let at = |secs: u64| t0 + Duration::from_secs(secs);
    b.app.sound_tick_at(now, at(3));
    b.app.sound_tick_at(now, at(8));
    assert_eq!(asked(&b).len(), 2, "asked once");
    b.app.sound_tick_at(now, at(11));
    assert_eq!(asked(&b).len(), 4, "asked again after 10 s");
    b.app.sound_tick_at(now, at(25));
    assert_eq!(asked(&b).len(), 4, "the next wait is 20 s");
    b.app.sound_tick_at(now, at(32));
    assert_eq!(asked(&b).len(), 6);

    // One file arrives, verified against its hash: Plan is told and the
    // status moves. Bytes that do not hash to the name are refused.
    let walk = BlobHash::of(b"walk-in music");
    let blobs = b.app.library().repo().blobs();
    assert!(blobs.put_verified(&walk, &b"not the music"[..]).is_err());
    assert!(blobs.path(&walk).is_none(), "a wrong file is not kept");
    blobs.put_verified(&walk, &b"walk-in music"[..]).unwrap();
    let heard = b.heard_sound();
    b.app.sound_tick_at(now, at(40));
    assert!(b.heard_sound() > heard, "Plan is told a file arrived");
    let status = b.status(&s);
    assert!(status.iter().any(|f| !f.backing && f.on_device));
    assert!(status.iter().any(|f| f.backing && !f.on_device && f.coming));
}

#[test]
fn only_a_few_files_are_outstanding_at_once() {
    let now_ms = real_now();
    let mut b = device(true);
    let s = b.service(ServiceRequest::Create {
        title: "Sunday".into(),
        starts_at: Some(Timestamp(now_ms + HOUR)),
        time_zone: None,
    });
    let mut s = s;
    for n in 0..6 {
        s = b.insert(
            &s,
            ItemKind::Sound {
                sound: sound(
                    &format!("Track {n}"),
                    format!("t{n}").as_bytes(),
                    Some(1),
                    SoundStart::Waits,
                ),
            },
        );
    }
    b.pump();
    b.app.sound_tick_at(Timestamp(now_ms), Instant::now());
    assert_eq!(asked(&b).len(), 4);
}

/// Which services are fetched: those that started less than 36 hours ago or
/// start later. An undated one is not, and neither is a past one.
#[test]
fn only_services_in_the_window_are_fetched() {
    let now_ms = real_now();
    let hours = |n: i64| now_ms + n * HOUR;
    for (starts, fetched) in [
        (Some(hours(2)), true),
        (Some(hours(-35)), true),
        (Some(hours(-36)), true), // exactly 36 h ago is still in
        (Some(hours(-36) - 1), false),
        (Some(hours(-24 * 30)), false),
        (None, false),
    ] {
        let mut b = device(true);
        let s = dated_with_sound(&mut b, starts, b"music");
        b.pump();
        b.app.sound_tick_at(Timestamp(now_ms), Instant::now());
        assert_eq!(!asked(&b).is_empty(), fetched, "starts {starts:?}");
        // Plan only says a missing file is coming for one that is fetched.
        if starts != Some(hours(-36)) {
            // (The edge itself depends on the millisecond Plan reads the clock.)
            assert_eq!(b.status(&s)[0].coming, fetched, "starts {starts:?}");
        }
        assert!(!b.status(&s)[0].on_device);
    }
}

#[test]
fn a_device_on_its_own_asks_for_nothing_reads_nothing_and_says_nothing_is_coming() {
    let now_ms = real_now();
    let mut alone = device(false);
    let s = dated_with_sound(&mut alone, Some(now_ms + HOUR), b"walk");
    alone.pump();
    let t0 = Instant::now();
    for secs in [0, 3, 30, 300] {
        alone
            .app
            .sound_tick_at(Timestamp(now_ms), t0 + Duration::from_secs(secs));
    }
    assert!(asked(&alone).is_empty());
    assert_eq!(alone.heard_sound(), 0);
    assert_eq!(alone.app.sound_scans(), 0, "no service was read for it");
    let status = alone.status(&s);
    assert!(
        !status[0].on_device && !status[0].coming,
        "nobody to send it"
    );
}

/// A church with other devices but no sound reads each service once when it
/// meets them, a few at a time, and then nothing again until one changes.
#[test]
fn a_church_without_sound_does_no_repeated_scan() {
    let now_ms = real_now();
    let mut d = device(true);
    for n in 0..20 {
        d.service(ServiceRequest::Create {
            title: format!("Service {n}"),
            starts_at: Some(Timestamp(now_ms + HOUR)),
            time_zone: None,
        });
    }
    d.pump();
    let t0 = Instant::now();
    let now = Timestamp(now_ms);
    // The first look reads at most eight services, so the live tick that
    // calls this never waits on all of them.
    d.app.sound_tick_at(now, t0);
    assert_eq!(d.app.sound_scans(), 8);
    d.app.sound_tick_at(now, t0 + Duration::from_secs(3));
    d.app.sound_tick_at(now, t0 + Duration::from_secs(6));
    assert_eq!(d.app.sound_scans(), 20, "each service once");
    for secs in [9, 20, 60, 600] {
        d.app.sound_tick_at(now, t0 + Duration::from_secs(secs));
    }
    assert_eq!(d.app.sound_scans(), 20, "nothing changed, nothing read");
    assert!(asked(&d).is_empty());
    assert_eq!(d.heard_sound(), 0);

    // One service changes: only that one is read.
    let first = match d.ok(Request::Service(ServiceRequest::List)) {
        Response::Services(s) => s[0].id,
        other => panic!("{other:?}"),
    };
    d.service(ServiceRequest::Rename {
        service: first,
        title: "Renamed".into(),
    });
    d.pump();
    d.app.sound_tick_at(now, t0 + Duration::from_secs(700));
    assert_eq!(d.app.sound_scans(), 21);
}

#[test]
fn a_church_that_does_not_use_sound_sees_nothing_new() {
    let now_ms = real_now();
    let mut d = device(true);
    let s = d.create();
    let s = d.insert(
        &s,
        ItemKind::Song {
            song: Song::new("x").id,
            arrangement: None,
        },
    );
    d.pump();
    d.app.sound_tick_at(Timestamp(now_ms), Instant::now());
    // Nothing asked for, nothing announced, no sound files to report, no
    // sound in the document or in what the UI is sent.
    assert!(asked(&d).is_empty());
    assert_eq!(d.heard_sound(), 0);
    assert!(d.status(&s).is_empty());
    assert!(!s.has_sound());
    let json = serde_json::to_string(&s).unwrap();
    assert!(
        !json.contains("backing") && !json.contains("Sound"),
        "{json}"
    );
    // The estimate has no sound either.
    assert!(d
        .estimates(&s)
        .iter()
        .all(|e| e.sound_ms.is_none() && !e.sound));
}

/// Every common audio container goes through `AddSoundFile`: name from the
/// file, a length near the real 1.5 s, and the file in the store. (The Ogg is
/// FLAC in an Ogg container; no Vorbis encoder was at hand.)
#[test]
fn mp3_flac_m4a_and_ogg_files_are_added_with_their_length() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sound");
    for ext in ["mp3", "flac", "m4a", "ogg"] {
        let mut d = device(false);
        let path = dir.join(format!("tone.{ext}"));
        let file = match d.ok(Request::Library(LibraryRequest::AddSoundFile {
            path: path.to_str().unwrap().into(),
        })) {
            Response::SoundFile(f) => f,
            other => panic!("{ext}: {other:?}"),
        };
        assert_eq!(file.name, "tone", "{ext}");
        let ms = file.length_ms.unwrap_or_else(|| panic!("{ext}: no length"));
        assert!((1300..=1800).contains(&ms), "{ext}: {ms} ms");
        assert!(d.app.library().repo().blobs().path(&file.file).is_some());
    }
}

/// A service document this version cannot read (a newer Lampstand's) does
/// not take the list down: it is left out, counted, and an update is held
/// back because its start time is unknown.
#[test]
fn a_service_from_a_newer_version_is_left_out_counted_and_holds_updates() {
    let now_ms = real_now();
    #[derive(autosurgeon::Hydrate, autosurgeon::Reconcile)]
    struct FromTheFuture {
        id: lampstand_core::service::ServiceId,
        title: String,
        starts_at: String,
    }
    impl lampstand_sync::Doc for FromTheFuture {
        const KIND: lampstand_sync::DocKind = lampstand_sync::DocKind::Service;
        fn doc_id(&self) -> Uuid {
            self.id.uuid()
        }
    }

    let mut d = device(false);
    d.create();
    // Nothing unreadable yet: nothing holds an update.
    assert_eq!(d.app.update_blocker_at(Timestamp(now_ms)), None);
    d.app
        .library()
        .repo()
        .put(&FromTheFuture {
            id: lampstand_core::service::ServiceId::generate(),
            title: "Newer".into(),
            starts_at: "tomorrow".into(),
        })
        .unwrap();
    match d.ok(Request::Service(ServiceRequest::List)) {
        Response::Services(list) => assert_eq!(list.len(), 1),
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        d.ok(Request::Service(ServiceRequest::Unreadable)),
        Response::Unreadable(1)
    ));
    // The media list reads every service too, and still answers.
    d.ok(Request::Library(LibraryRequest::ListMedia));
    let why = d
        .app
        .update_blocker_at(Timestamp(now_ms))
        .expect("held back");
    assert!(why.contains("newer Lampstand"), "{why}");
}

/// The Setup switch is off until somebody turns it on, is this device's own,
/// and survives other settings being saved.
#[test]
fn the_sound_switch_is_off_by_default_and_kept_with_the_device_settings() {
    let mut d = device(false);
    let get = |d: &mut Device| match d.ok(Request::Settings(SettingsRequest::Get)) {
        Response::Settings(s) => s,
        other => panic!("{other:?}"),
    };
    let mut settings = get(&mut d);
    assert!(!settings.use_sound, "off by default");
    settings.use_sound = true;
    d.ok(Request::Settings(SettingsRequest::Put(settings)));
    let mut settings = get(&mut d);
    assert!(settings.use_sound);
    // Saving something else with the switch as read keeps it.
    settings.clicker_mode = true;
    d.ok(Request::Settings(SettingsRequest::Put(settings)));
    assert!(get(&mut d).use_sound);
}
