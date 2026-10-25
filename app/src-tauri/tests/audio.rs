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
    problem_words, App, AudioRemote, Placement, SyncLink, WindowHost, AUDIO_AWAY, AUDIO_NOBODY,
    AUDIO_NOT_ALLOWED,
};
use lampstand_core::id::BlobHash;
use lampstand_core::output::OutputId;
use lampstand_core::service::{ItemId, ItemKind, Service, Sound, SoundStart};
use lampstand_core::song::{Section, Song};
use lampstand_layout::Fonts;
use lampstand_live::{Command, Cursor};
use lampstand_media::{OutputState, Pcm, Playing};
use lampstand_render::output::DisplayInfo;
use lampstand_render::{OutputCommand, OutputSink};
use lampstand_sync::net::{SoundCommand, SoundNow, SoundProblem, SoundReason, SoundRoute};

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
    /// `None` is working; a test sets it to make the output die.
    state: Option<OutputState>,
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
    fn state(&self) -> OutputState {
        self.0.lock().unwrap().state.unwrap_or(OutputState::Open)
    }
}

/// A sync link whose Audio holder is whatever the test says.
struct Link {
    route: Mutex<SoundRoute>,
    here: Mutex<bool>,
    remote: Mutex<Option<AudioRemote>>,
    sent: Mutex<Vec<SoundCommand>>,
    reported: Mutex<Vec<Option<SoundNow>>>,
    problems: Mutex<Vec<Option<SoundProblem>>>,
}

impl Link {
    fn new(route: SoundRoute, here: bool) -> Arc<Link> {
        Arc::new(Link {
            route: Mutex::new(route),
            here: Mutex::new(here),
            remote: Mutex::new(None),
            sent: Mutex::new(Vec::new()),
            reported: Mutex::new(Vec::new()),
            problems: Mutex::new(Vec::new()),
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
    fn report_sound_problem(&self, problem: Option<SoundProblem>) {
        self.problems.lock().unwrap().push(problem);
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
        Some(
            "Walk-in music couldn't be played. The sound file isn't on this computer yet. \
             Add the file again, or wait for it to come from another computer."
        )
    );
    assert_eq!(
        v.problem_item,
        Some(item.to_string()),
        "shown next to the item"
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

/// A service of a song with lyrics, a music-only item and another song,
/// opened: the service and the ids of the three.
fn song_music_song(d: &mut Device, start: SoundStart) -> (Service, [ItemId; 3]) {
    let mut song = Song::new("Amazing Grace");
    song.sections.push(Section {
        label: "V1".into(),
        lines: vec!["Amazing grace, how sweet the sound".into()],
    });
    d.app.library().put_song(&song).unwrap();
    let mut s = d.service(ServiceRequest::Create {
        title: "Sunday".into(),
        starts_at: None,
        time_zone: None,
    });
    for kind in [
        ItemKind::Song {
            song: song.id,
            arrangement: None,
        },
        ItemKind::Sound {
            sound: sound("Offering music", WALK, start),
        },
        ItemKind::Song {
            song: song.id,
            arrangement: None,
        },
    ] {
        s = d.service(ServiceRequest::InsertItem {
            service: s.id,
            index: usize::MAX,
            kind,
        });
    }
    d.ok(Request::Service(ServiceRequest::Open(s.id)));
    let ids: Vec<ItemId> = s.ordered().into_iter().map(|(id, _)| id).collect();
    (s, [ids[0], ids[1], ids[2]])
}

/// Presses Next: the item live after it, as the screen is told.
fn live_item(d: &mut Device) -> Option<ItemId> {
    d.ok(Request::Live(Command::Next));
    d.events
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find_map(|e| match e {
            Event::Live { live, .. } => Some(live.map(|c| c.item)),
            _ => None,
        })?
}

#[test]
fn next_onto_music_that_plays_on_its_own_starts_it() {
    let mut d = device();
    let (_, [song, music, after]) = song_music_song(&mut d, SoundStart::OnItsOwn);
    assert_eq!(live_item(&mut d), Some(song));
    assert!(d.calls().is_empty());
    assert_eq!(live_item(&mut d), Some(music));
    assert_eq!(d.calls(), ["play@0"]);
    let v = d.sound_request(SoundRequest::Status);
    assert_eq!(playing(&v).name, "Offering music");
    // The music plays on under the next item.
    assert_eq!(live_item(&mut d), Some(after));
    assert_eq!(d.calls(), ["play@0"]);
}

#[test]
fn next_onto_music_that_waits_does_not_start_it() {
    let mut d = device();
    let (_, [_, music, _]) = song_music_song(&mut d, SoundStart::Waits);
    live_item(&mut d);
    assert_eq!(live_item(&mut d), Some(music));
    assert!(d.calls().is_empty(), "it waits for Play");
    let v = d.sound_request(SoundRequest::Play {
        item: music,
        backing: false,
    });
    assert_eq!(playing(&v).name, "Offering music");
}

#[test]
fn music_that_starts_on_its_own_lets_what_plays_fade_out_first() {
    let mut d = device();
    let (s, [song, music, _]) = song_music_song(&mut d, SoundStart::OnItsOwn);
    d.service(ServiceRequest::SetSound {
        service: s.id,
        item: song,
        slot: SoundSlot::Backing,
        sound: Some(sound("Amazing Grace (track)", WALK, SoundStart::Waits)),
    });
    live_item(&mut d);
    d.sound_request(SoundRequest::Play {
        item: song,
        backing: true,
    });
    assert_eq!(d.calls(), ["play@0"]);
    assert_eq!(live_item(&mut d), Some(music));
    assert_eq!(
        d.calls(),
        ["play@0", "stop 1"],
        "the track fades; the music waits for it"
    );
    // The fade ends: the music starts.
    d.log.lock().unwrap().playing = None;
    let v = d.sound_request(SoundRequest::Status);
    assert_eq!(d.calls(), ["play@0", "stop 1", "play@0"]);
    assert_eq!(playing(&v).name, "Offering music");
}

#[test]
fn next_onto_music_sends_the_start_to_the_audio_holder_on_another_device() {
    let mut d = device();
    let link = Link::new(SoundRoute::Sent, false);
    let desk = |playing: Option<SoundNow>| AudioRemote {
        name: "Sound desk".into(),
        connected: true,
        playing,
        problem: None,
    };
    *link.remote.lock().unwrap() = Some(desk(None));
    d.app.set_sync_link(link.clone());
    let (_, [_, music, _]) = song_music_song(&mut d, SoundStart::OnItsOwn);
    let play = SoundCommand::Play {
        file: *BlobHash::of(WALK).as_bytes(),
        name: "Offering music".into(),
        item: music.to_string(),
    };
    live_item(&mut d);
    assert_eq!(live_item(&mut d), Some(music));
    assert_eq!(*link.sent.lock().unwrap(), std::slice::from_ref(&play));
    assert!(d.calls().is_empty(), "it plays on the desk, not here");

    // With something playing on the desk, the fade goes first.
    *link.remote.lock().unwrap() = Some(desk(Some(SoundNow {
        item: "item-9".into(),
        name: "Walk-in music".into(),
        paused: false,
        ending: false,
        position_ms: 4000,
        length_ms: 90_000,
    })));
    link.sent.lock().unwrap().clear();
    d.ok(Request::Live(Command::Previous));
    d.ok(Request::Live(Command::Next));
    assert_eq!(
        *link.sent.lock().unwrap(),
        [SoundCommand::Stop { fade_ms: 1000 }, play]
    );
}

#[test]
fn a_cue_goes_to_the_audio_holder_when_it_is_another_device() {
    let mut d = device();
    let link = Link::new(SoundRoute::Sent, false);
    *link.remote.lock().unwrap() = Some(AudioRemote {
        name: "Sound desk".into(),
        connected: true,
        playing: None,
        problem: None,
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
        problem: None,
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
        problem: None,
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

fn trouble(reason: SoundReason, item: &str) -> SoundProblem {
    SoundProblem {
        reason,
        item: item.into(),
        name: "Walk-in music".into(),
    }
}

#[test]
fn the_operator_is_told_why_a_sound_cannot_play_on_the_audio_holder() {
    let mut d = device();
    let link = Link::new(SoundRoute::Sent, false);
    d.app.set_sync_link(link.clone());
    let (_, item) = d.music_service(WALK, SoundStart::Waits);
    let item = item.to_string();
    for (reason, expected) in [
        (
            SoundReason::NotOnDevice,
            "Walk-in music couldn't be played. The sound file isn't on Sound desk yet. \
             Wait for it to arrive, or add the file again.",
        ),
        (
            SoundReason::Unsupported,
            "Walk-in music couldn't be played. Sound desk can't play this kind of file. \
             Lampstand can play MP3, WAV, FLAC, M4A and Ogg.",
        ),
        (
            SoundReason::Unreadable,
            "Walk-in music couldn't be played. Sound desk couldn't read the sound file. \
             It may be damaged. Try adding it to the service again.",
        ),
        (
            SoundReason::OutputDown,
            "Walk-in music isn't playing. The sound output on Sound desk isn't working. \
             Check it is plugged in and switched on.",
        ),
        (
            SoundReason::OutputMissing,
            "Walk-in music isn't playing. The sound output chosen on Sound desk isn't connected. \
             Plug it in, or choose another in Setup \u{203a} Sound.",
        ),
        (
            SoundReason::NoOutput,
            "Walk-in music isn't playing. Sound desk has no sound output. \
             Plug one in and switch it on.",
        ),
    ] {
        *link.remote.lock().unwrap() = Some(AudioRemote {
            name: "Sound desk".into(),
            connected: true,
            playing: None,
            problem: Some(trouble(reason, &item)),
        });
        let v = d.sound_request(SoundRequest::Status);
        assert_eq!(v.problem.as_deref(), Some(expected), "{reason:?}");
        assert_eq!(v.problem_item.as_deref(), Some(item.as_str()));
        assert!(!v.here, "this is the Operator's view");
    }
    // Nothing wrong, nothing said.
    *link.remote.lock().unwrap() = Some(AudioRemote {
        name: "Sound desk".into(),
        connected: true,
        playing: None,
        problem: None,
    });
    let v = d.sound_request(SoundRequest::Status);
    assert!(v.problem.is_none() && v.problem_item.is_none());
    // The holder being away is said as away, not as a sound problem.
    *link.remote.lock().unwrap() = Some(AudioRemote {
        name: "Sound desk".into(),
        connected: false,
        playing: None,
        problem: Some(trouble(SoundReason::OutputDown, &item)),
    });
    let v = d.sound_request(SoundRequest::Status);
    assert!(v.away && v.problem.is_none());
}

#[test]
fn a_problem_with_no_sound_named_reads_as_one_sentence_and_names_no_path() {
    let p = SoundProblem {
        reason: SoundReason::OutputDown,
        item: String::new(),
        name: String::new(),
    };
    assert_eq!(
        problem_words(&p, Some("Sound desk")),
        "The sound output on Sound desk isn't working. Check it is plugged in and switched on."
    );
    assert_eq!(
        problem_words(&p, None),
        "The sound output on this computer isn't working. Check it is plugged in and \
         switched on, or choose another in Setup \u{203a} Sound."
    );
}

#[test]
fn a_dead_output_on_a_one_device_church_is_said_once_and_goes_when_it_comes_back() {
    let mut d = device();
    let (_, item) = d.music_service(WALK, SoundStart::Waits);
    let v = d.sound_request(SoundRequest::Play {
        item,
        backing: false,
    });
    assert!(v.output_ok && v.problem.is_none());
    // The interface is unplugged mid-play.
    d.log.lock().unwrap().state = Some(OutputState::Failed);
    d.app.tick();
    let v = d.sound_request(SoundRequest::Status);
    assert!(v.playing.is_none(), "not playing, not 'PLAYING' forever");
    assert!(!v.output_ok);
    assert_eq!(
        v.problem.as_deref(),
        Some(
            "Walk-in music isn't playing. The sound output on this computer isn't working. \
             Check it is plugged in and switched on, or choose another in Setup \u{203a} Sound."
        )
    );
    assert_eq!(v.problem_item, Some(item.to_string()));
    // Many ticks later it is still one message, not a new one each time.
    for _ in 0..20 {
        d.app.tick();
    }
    let with_problem = d
        .sound_events()
        .iter()
        .filter(|v| v.problem.is_some())
        .count();
    assert_eq!(with_problem, 1, "the screen is told once");
    // Pause, fade out and the panic stop answer with a clear screen.
    for r in [SoundRequest::Pause, SoundRequest::Stop, SoundRequest::Panic] {
        assert!(d.sound_request(r).playing.is_none());
    }
    // The interface is back and Play is pressed: it plays, and the message
    // goes.
    d.log.lock().unwrap().state = None;
    let v = d.sound_request(SoundRequest::Play {
        item,
        backing: false,
    });
    assert_eq!(playing(&v).name, "Walk-in music");
    d.app.tick();
    let v = d.sound_request(SoundRequest::Status);
    assert!(v.output_ok && v.problem.is_none() && v.problem_item.is_none());
    assert!(d.sound_events().last().unwrap().problem.is_none());
}

#[test]
fn the_audio_holder_sends_why_a_sound_did_not_play_to_the_others() {
    let mut d = device();
    let link = Link::new(SoundRoute::Local, true);
    d.app.set_sync_link(link.clone());
    let (_, item) = d.music_service(b"never fetched", SoundStart::Waits);
    d.sound_request(SoundRequest::Play {
        item,
        backing: false,
    });
    let said = link.problems.lock().unwrap().clone();
    assert_eq!(
        said.last().unwrap().as_ref().map(|p| p.reason),
        Some(SoundReason::NotOnDevice)
    );
    assert_eq!(
        said.last().unwrap().as_ref().unwrap().item,
        item.to_string()
    );
    let count = said.len();
    d.app.tick();
    d.app.tick();
    assert_eq!(link.problems.lock().unwrap().len(), count, "said once");
    // The next sound that plays ends it.
    let (_, ok) = d.music_service(WALK, SoundStart::Waits);
    d.sound_request(SoundRequest::Play {
        item: ok,
        backing: false,
    });
    assert_eq!(link.problems.lock().unwrap().last(), Some(&None));
}

#[test]
fn a_church_that_never_uses_sound_sends_no_problem_anywhere() {
    let mut d = device();
    let link = Link::new(SoundRoute::Local, true);
    d.app.set_sync_link(link.clone());
    for _ in 0..5 {
        d.app.tick();
    }
    assert!(link.problems.lock().unwrap().iter().all(|p| p.is_none()));
    assert!(d.sound_events().iter().all(|v| v.problem.is_none()));
    assert!(d.opened.lock().unwrap().is_empty());
}

#[test]
fn the_formats_named_in_the_words_are_the_ones_the_decoder_plays() {
    use lampstand_media::PLAYABLE_EXTENSIONS;
    let named = lampstand_app::handler::FORMATS;
    for (word, ext) in [
        ("MP3", "mp3"),
        ("WAV", "wav"),
        ("FLAC", "flac"),
        ("M4A", "m4a"),
        ("Ogg", "ogg"),
    ] {
        assert!(named.contains(word), "{word}");
        assert!(PLAYABLE_EXTENSIONS.contains(&ext), "{ext}");
    }
}
