//! The Audio role in the app (decision 0010): asking for sound, where it
//! plays, and what the screens are told. A fake output stands in for the
//! sound device and a fake link for the sync node.

use std::sync::{Arc, Mutex};

use lampstand_api::{
    Event, Request, Response, ServiceRequest, SettingsRequest, SoundPlaying, SoundRequest,
    SoundSlot, SoundView,
};
use lampstand_app::audio::{AudioDeck, Out, Parts};
use lampstand_app::handler::{
    App, AudioRemote, Placement, SyncLink, WindowHost, AUDIO_AWAY, AUDIO_NOBODY, AUDIO_NOT_ALLOWED,
};
use lampstand_core::id::BlobHash;
use lampstand_core::output::OutputId;
use lampstand_core::service::{ItemId, ItemKind, Service, Sound, SoundStart};
use lampstand_core::song::{Section, Song};
use lampstand_layout::Fonts;
use lampstand_live::{Command, Cursor};
use lampstand_media::{Pcm, Playing};
use lampstand_render::output::DisplayInfo;
use lampstand_render::{OutputCommand, OutputSink};
use lampstand_sync::net::{SoundCommand, SoundNow, SoundRoute};

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

#[derive(Default)]
struct Log {
    calls: Vec<String>,
    playing: Option<Playing>,
}

struct Fake(Arc<Mutex<Log>>);

impl Out for Fake {
    fn play(&mut self, _: Arc<Pcm>, from: f64) {
        let mut l = self.0.lock().unwrap();
        l.calls.push(format!("play@{from}"));
        l.playing = Some(Playing {
            paused: false,
            ending: false,
            position: 0.0,
            length: 30.0,
        });
    }
    fn pause(&mut self) {
        let mut l = self.0.lock().unwrap();
        l.calls.push("pause".into());
        if let Some(p) = l.playing.as_mut() {
            p.paused = true;
        }
    }
    fn resume(&mut self) {
        let mut l = self.0.lock().unwrap();
        l.calls.push("resume".into());
        if let Some(p) = l.playing.as_mut() {
            p.paused = false;
        }
    }
    fn stop(&mut self, secs: f64) {
        let mut l = self.0.lock().unwrap();
        l.calls.push(format!("stop {secs}"));
        if let Some(p) = l.playing.as_mut() {
            p.ending = true;
        }
    }
    fn panic(&mut self) {
        let mut l = self.0.lock().unwrap();
        l.calls.push("panic".into());
        l.playing = None;
    }
    fn playing(&mut self) -> Option<Playing> {
        self.0.lock().unwrap().playing
    }
    fn open(&self) -> bool {
        true
    }
}

/// A sync link whose Audio holder is whatever the test says.
struct Link {
    route: Mutex<SoundRoute>,
    here: Mutex<bool>,
    remote: Mutex<Option<AudioRemote>>,
    sent: Mutex<Vec<SoundCommand>>,
    reported: Mutex<Vec<Option<SoundNow>>>,
}

impl Link {
    fn new(route: SoundRoute, here: bool) -> Arc<Link> {
        Arc::new(Link {
            route: Mutex::new(route),
            here: Mutex::new(here),
            remote: Mutex::new(None),
            sent: Mutex::new(Vec::new()),
            reported: Mutex::new(Vec::new()),
        })
    }
}

impl SyncLink for Link {
    fn device_name(&self) -> String {
        "Test device".into()
    }
    fn device_key(&self) -> String {
        "a".into()
    }
    fn connected(&self) -> Vec<String> {
        vec!["peer".into()]
    }
    fn fetch(&self, _: Vec<BlobHash>) {}
    fn route_sound(&self, cmd: SoundCommand) -> SoundRoute {
        let route = *self.route.lock().unwrap();
        if route == SoundRoute::Sent {
            self.sent.lock().unwrap().push(cmd);
        }
        route
    }
    fn report_sound(&self, now: Option<SoundNow>) {
        self.reported.lock().unwrap().push(now);
    }
    fn audio_here(&self) -> bool {
        *self.here.lock().unwrap()
    }
    fn audio_remote(&self) -> Option<AudioRemote> {
        self.remote.lock().unwrap().clone()
    }
}

const WALK: &[u8] = b"walk-in music";

struct Device {
    app: App,
    log: Arc<Mutex<Log>>,
    opened: Arc<Mutex<Vec<Option<String>>>>,
    events: Arc<Mutex<Vec<Event>>>,
    _dir: tempfile::TempDir,
}

fn device() -> Device {
    let dir = tempfile::tempdir().unwrap();
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
    // The files that are "here": the walk-in music, put in the blob store.
    let hash = app.library().repo().blobs().put(WALK).unwrap();
    let log = Arc::new(Mutex::new(Log::default()));
    let opened = Arc::new(Mutex::new(Vec::new()));
    let (l, o) = (log.clone(), opened.clone());
    let library = app.library().clone();
    let parts = Parts {
        open: Box::new(move |name| {
            o.lock().unwrap().push(name);
            Box::new(Fake(l.clone()))
        }),
        path: Box::new(move |h: &BlobHash| library.repo().blobs().path(h)),
        decode: Arc::new(|_: &std::path::Path| {
            Ok(Arc::new(Pcm {
                rate: 10,
                channels: 1,
                samples: vec![0.0; 10],
            }))
        }),
        spawn: Arc::new(|job| job()),
    };
    assert_eq!(hash, BlobHash::of(WALK));
    app.set_audio(AudioDeck::new(parts, None));
    Device {
        app,
        log,
        opened,
        events,
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

    fn err(&mut self, request: Request) -> String {
        match self.app.handle(request) {
            Response::Error { message } => message,
            other => panic!("expected an error, got {other:?}"),
        }
    }

    fn service(&mut self, request: ServiceRequest) -> Service {
        match self.ok(Request::Service(request)) {
            Response::Service(s) => s,
            other => panic!("{other:?}"),
        }
    }

    fn sound_request(&mut self, request: SoundRequest) -> SoundView {
        match self.ok(Request::Sound(request)) {
            Response::Sound(v) => v,
            other => panic!("{other:?}"),
        }
    }

    fn calls(&self) -> Vec<String> {
        self.log.lock().unwrap().calls.clone()
    }

    fn sound_events(&self) -> Vec<SoundView> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                Event::Sound(v) => Some((**v).clone()),
                _ => None,
            })
            .collect()
    }

    /// A service with one music-only item, opened; its item id.
    fn music_service(&mut self, hash_of: &[u8], start: SoundStart) -> (Service, ItemId) {
        let s = self.service(ServiceRequest::Create {
            title: "Sunday".into(),
            starts_at: None,
            time_zone: None,
        });
        let s = self.service(ServiceRequest::InsertItem {
            service: s.id,
            index: usize::MAX,
            kind: ItemKind::Sound {
                sound: sound("Walk-in music", hash_of, start),
            },
        });
        let (item, _) = s
            .ordered()
            .into_iter()
            .map(|(i, x)| (i, x.kind.clone()))
            .next()
            .unwrap();
        self.ok(Request::Service(ServiceRequest::Open(s.id)));
        (s, item)
    }
}

fn sound(name: &str, bytes: &[u8], start: SoundStart) -> Sound {
    Sound {
        name: name.into(),
        file: BlobHash::of(bytes),
        length_ms: Some(30_000),
        start,
        keep_off_stream: false,
    }
}

fn playing(v: &SoundView) -> &SoundPlaying {
    v.playing.as_ref().expect("something is playing")
}

#[test]
fn a_music_only_item_plays_here_when_this_device_is_alone_and_can_be_paused_stopped_and_cut() {
    let mut d = device();
    let (_, item) = d.music_service(WALK, SoundStart::Waits);
    let v = d.sound_request(SoundRequest::Status);
    assert!(v.here && v.holder.is_empty() && !v.nobody && v.playing.is_none());
    assert!(
        d.opened.lock().unwrap().is_empty(),
        "no output until needed"
    );

    let v = d.sound_request(SoundRequest::Play {
        item,
        backing: false,
    });
    assert_eq!(playing(&v).name, "Walk-in music");
    assert_eq!(playing(&v).item, item.to_string());
    assert_eq!(playing(&v).length_ms, 30_000);
    assert_eq!(d.calls(), ["play@0"]);

    let v = d.sound_request(SoundRequest::Pause);
    assert!(playing(&v).paused);
    let v = d.sound_request(SoundRequest::Resume);
    assert!(!playing(&v).paused);
    let v = d.sound_request(SoundRequest::Stop);
    assert!(playing(&v).ending);
    assert!(d.calls().contains(&"stop 3".to_string()));
    let v = d.sound_request(SoundRequest::Panic);
    assert!(v.playing.is_none());
    // The screen was told along the way.
    let told = d.sound_events();
    assert!(told
        .iter()
        .any(|v| v.playing.as_ref().is_some_and(|p| !p.paused)));
    assert!(told.last().unwrap().playing.is_none());
}

#[test]
fn a_file_that_is_not_on_this_device_says_so_in_words() {
    let mut d = device();
    let (_, item) = d.music_service(b"never fetched", SoundStart::Waits);
    let v = d.sound_request(SoundRequest::Play {
        item,
        backing: false,
    });
    assert!(v.playing.is_none());
    assert_eq!(
        v.problem.as_deref(),
        Some("Walk-in music is not on this device yet.")
    );
    assert!(d.opened.lock().unwrap().is_empty());
}

#[test]
fn asking_for_an_item_with_no_sound_is_refused() {
    let mut d = device();
    let (s, item) = d.music_service(WALK, SoundStart::Waits);
    assert_eq!(
        d.err(Request::Sound(SoundRequest::Play {
            item,
            backing: true
        })),
        "that item has no sound"
    );
    assert_eq!(
        d.err(Request::Sound(SoundRequest::Play {
            item: ItemId::generate(),
            backing: false
        })),
        "no such item"
    );
    let _ = s;
}

/// A service with one song that has lyrics, so it can go live, and a
/// backing track.
fn song_with_backing(d: &mut Device, start: SoundStart) -> ItemId {
    let mut song = Song::new("Amazing Grace");
    song.sections.push(Section {
        label: "V1".into(),
        lines: vec!["Amazing grace, how sweet the sound".into()],
    });
    d.app.library().put_song(&song).unwrap();
    let s = d.service(ServiceRequest::Create {
        title: "Sunday".into(),
        starts_at: None,
        time_zone: None,
    });
    let s = d.service(ServiceRequest::InsertItem {
        service: s.id,
        index: usize::MAX,
        kind: ItemKind::Song {
            song: song.id,
            arrangement: None,
        },
    });
    let (item, _) = s.ordered().into_iter().next().unwrap();
    d.service(ServiceRequest::SetSound {
        service: s.id,
        item,
        slot: SoundSlot::Backing,
        sound: Some(sound("Amazing Grace (track)", WALK, start)),
    });
    d.ok(Request::Service(ServiceRequest::Open(s.id)));
    item
}

#[test]
fn a_backing_track_that_plays_on_its_own_starts_when_the_song_goes_live() {
    let mut d = device();
    let item = song_with_backing(&mut d, SoundStart::OnItsOwn);
    assert!(d.calls().is_empty());
    d.ok(Request::Live(Command::Show(Cursor {
        item,
        slide: 0,
        step: 0,
    })));
    assert_eq!(d.calls(), ["play@0"]);
    let v = d.sound_request(SoundRequest::Status);
    assert_eq!(playing(&v).name, "Amazing Grace (track)");
}

#[test]
fn a_backing_track_that_waits_does_not_start_until_play_is_pressed() {
    let mut d = device();
    let item = song_with_backing(&mut d, SoundStart::Waits);
    d.ok(Request::Live(Command::Show(Cursor {
        item,
        slide: 0,
        step: 0,
    })));
    assert!(d.calls().is_empty());
    let v = d.sound_request(SoundRequest::Play {
        item,
        backing: true,
    });
    assert_eq!(playing(&v).name, "Amazing Grace (track)");
}

#[test]
fn a_cue_goes_to_the_audio_holder_when_it_is_another_device() {
    let mut d = device();
    let link = Link::new(SoundRoute::Sent, false);
    *link.remote.lock().unwrap() = Some(AudioRemote {
        name: "Sound desk".into(),
        connected: true,
        playing: None,
    });
    d.app.set_sync_link(link.clone());
    let (_, item) = d.music_service(WALK, SoundStart::Waits);
    let v = d.sound_request(SoundRequest::Play {
        item,
        backing: false,
    });
    assert_eq!(
        *link.sent.lock().unwrap(),
        [SoundCommand::Play {
            file: *BlobHash::of(WALK).as_bytes(),
            name: "Walk-in music".into(),
            item: item.to_string(),
        }]
    );
    assert!(d.calls().is_empty(), "it plays on the desk, not here");
    assert!(!v.here && v.holder == "Sound desk" && !v.away && !v.nobody);
    d.sound_request(SoundRequest::Pause);
    d.sound_request(SoundRequest::Stop);
    d.sound_request(SoundRequest::Panic);
    assert_eq!(
        link.sent.lock().unwrap()[1..],
        [
            SoundCommand::Pause,
            SoundCommand::Stop { fade_ms: 3000 },
            SoundCommand::Panic
        ]
    );
}

#[test]
fn a_cue_that_cannot_go_anywhere_says_why_in_words() {
    let mut d = device();
    let link = Link::new(SoundRoute::Away, false);
    d.app.set_sync_link(link.clone());
    let (_, item) = d.music_service(WALK, SoundStart::Waits);
    let play = Request::Sound(SoundRequest::Play {
        item,
        backing: false,
    });
    for (route, words) in [
        (SoundRoute::Away, AUDIO_AWAY),
        (SoundRoute::Nobody, AUDIO_NOBODY),
        (SoundRoute::NotAllowed, AUDIO_NOT_ALLOWED),
    ] {
        *link.route.lock().unwrap() = route;
        assert_eq!(d.err(play.clone()), words);
        assert_eq!(d.err(Request::Sound(SoundRequest::Panic)), words);
    }
    assert!(d.calls().is_empty());
    assert_eq!(
        AUDIO_AWAY,
        "The device that runs the sound is not connected."
    );
}

#[test]
fn the_screens_know_when_the_audio_holder_is_away_or_nobody() {
    let mut d = device();
    let link = Link::new(SoundRoute::Away, false);
    d.app.set_sync_link(link.clone());
    *link.remote.lock().unwrap() = Some(AudioRemote {
        name: "Sound desk".into(),
        connected: false,
        playing: None,
    });
    let v = d.sound_request(SoundRequest::Status);
    assert!(v.away && !v.nobody && v.holder == "Sound desk");
    *link.remote.lock().unwrap() = None;
    let v = d.sound_request(SoundRequest::Status);
    assert!(v.nobody && !v.away && v.holder.is_empty() && !v.here);
}

#[test]
fn what_the_audio_holder_plays_is_shown_here_from_what_it_said() {
    let mut d = device();
    let link = Link::new(SoundRoute::Sent, false);
    *link.remote.lock().unwrap() = Some(AudioRemote {
        name: "Sound desk".into(),
        connected: true,
        playing: Some(SoundNow {
            item: "item-9".into(),
            name: "Walk-in music".into(),
            paused: true,
            ending: false,
            position_ms: 4000,
            length_ms: 90_000,
        }),
    });
    d.app.set_sync_link(link);
    let v = d.sound_request(SoundRequest::Status);
    let p = playing(&v);
    assert_eq!(
        (p.name.as_str(), p.paused, p.position_ms),
        ("Walk-in music", true, 4000)
    );
    assert!(v.problem.is_none());
}

#[test]
fn the_audio_holder_tells_the_others_when_what_plays_changes_and_stops_when_it_loses_audio() {
    let mut d = device();
    let link = Link::new(SoundRoute::Local, true);
    d.app.set_sync_link(link.clone());
    let (_, item) = d.music_service(WALK, SoundStart::Waits);
    d.sound_request(SoundRequest::Play {
        item,
        backing: false,
    });
    d.app.tick();
    let told = link.reported.lock().unwrap().clone();
    assert!(told
        .last()
        .unwrap()
        .as_ref()
        .is_some_and(|n| n.name == "Walk-in music"));
    let count = told.len();
    // Ticks with nothing new say nothing new.
    d.app.tick();
    d.app.tick();
    assert_eq!(link.reported.lock().unwrap().len(), count);
    // Audio moves away: the sound stops at once.
    *link.here.lock().unwrap() = false;
    d.app.tick();
    assert!(d.calls().contains(&"panic".to_string()));
    let v = d.sound_request(SoundRequest::Status);
    assert!(v.playing.is_none() && v.nobody);
}

#[test]
fn the_output_chosen_in_setup_is_the_one_played_through() {
    let mut d = device();
    let mut settings = match d.ok(Request::Settings(SettingsRequest::Get)) {
        Response::Settings(s) => s,
        other => panic!("{other:?}"),
    };
    assert_eq!(settings.sound_output, None);
    settings.sound_output = Some("Mixer USB".into());
    d.ok(Request::Settings(SettingsRequest::Put(settings.clone())));
    // Kept, and kept across a save that does not mention it.
    let got = match d.ok(Request::Settings(SettingsRequest::Get)) {
        Response::Settings(s) => s,
        other => panic!("{other:?}"),
    };
    assert_eq!(got.sound_output.as_deref(), Some("Mixer USB"));
    let (_, item) = d.music_service(WALK, SoundStart::Waits);
    d.sound_request(SoundRequest::Play {
        item,
        backing: false,
    });
    assert_eq!(*d.opened.lock().unwrap(), [Some("Mixer USB".to_string())]);
    // Choosing another stops what plays.
    settings.sound_output = None;
    d.ok(Request::Settings(SettingsRequest::Put(settings)));
    let v = d.sound_request(SoundRequest::Status);
    assert!(v.playing.is_none());
}

#[test]
fn the_outputs_on_this_computer_are_listed_for_setup() {
    let mut d = device();
    match d.ok(Request::Sound(SoundRequest::Outputs)) {
        Response::SoundOutputs(o) => {
            assert_eq!(o.chosen, None);
            // A build machine may have none; the list is whatever there is.
            let _ = (o.devices, o.default);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_church_that_never_uses_sound_hears_nothing_new() {
    let mut d = device();
    for _ in 0..3 {
        d.app.tick();
    }
    // The screen is told once (the first state) and then nothing, and no
    // output was ever opened.
    assert!(d.sound_events().len() <= 1);
    assert!(d.opened.lock().unwrap().is_empty());
    assert!(d
        .sound_events()
        .iter()
        .all(|v| v.playing.is_none() && v.problem.is_none()));
}
