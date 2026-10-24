//! Service device → presenting machine, end to end: two apps with their
//! own data folders pair over the real local server and sync node, and a
//! lyric fix on one reloads the open service on the other without changing
//! what is on air. Times the edit to the reload.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use lampstand_api::{
    Event, LibraryRequest, LoadedService, OutputRequest, PresenterRole, Request, Response,
    ServiceRequest, TeamRequest,
};
use lampstand_app::config::device_key;
use lampstand_app::handler::{App, Placement, WindowHost};
use lampstand_app::sync::{mirroring, DriveSinks, NodeCell, Obs, Sync, SyncConfig};
use lampstand_core::output::{BackgroundMode, OutputConfig, OutputId, OutputRole, OutputTarget};
use lampstand_core::safe_area::SafeArea;
use lampstand_core::service::{ItemKind, Service};
use lampstand_core::song::{Song, SongId};
use lampstand_layout::Fonts;
use lampstand_live::{Command, Cursor};
use lampstand_render::output::DisplayInfo;
use lampstand_render::{OutputCommand, OutputSink};
use lampstand_sync::DocChanged;

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

/// One copy of the app: its state, its sync and what it emitted.
struct Device {
    app: App,
    sync: Sync,
    events: Arc<Mutex<Vec<Event>>>,
    changes: std::sync::mpsc::Receiver<DocChanged>,
    /// Role changes sync has reported, for `pump` to hand to the app as
    /// the shell's app thread does.
    roles: Arc<Mutex<Vec<bool>>>,
    /// Requests a driver sent and "a device started driving" notices, for
    /// `pump` to hand to the app as the shell's app thread does.
    driven: Arc<Mutex<Vec<Request>>>,
    announce: Arc<std::sync::atomic::AtomicBool>,
    _dir: tempfile::TempDir,
}

async fn device(name: &str) -> Device {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let recorded = events.clone();
    let cell = NodeCell::default();
    let emit: lampstand_app::handler::EventSink = mirroring(
        Arc::new(move |e| recorded.lock().unwrap().push(e)),
        cell.clone(),
    );
    let app = App::open(
        dir.path(),
        Fonts::bundled_only(),
        Arc::new(Sink),
        Box::new(Host),
        emit.clone(),
    )
    .unwrap();
    let changes = app.library().subscribe();
    let roles = Arc::new(Mutex::new(Vec::new()));
    let told = roles.clone();
    let driven = Arc::new(Mutex::new(Vec::new()));
    let queued = driven.clone();
    let announce = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let started = announce.clone();
    let sync = Sync::start(
        app.library(),
        device_key(dir.path()).unwrap(),
        dir.path(),
        SyncConfig {
            name: name.into(),
            port: 0,
            mdns: false,
        },
        Obs {
            hub: app.scene_hub().clone(),
            renderer: None,
        },
        emit,
        Arc::new(move |presenting| told.lock().unwrap().push(presenting)),
        DriveSinks {
            command: Arc::new(move |request| queued.lock().unwrap().push(request)),
            driver: Arc::new(move |on| {
                if on {
                    started.store(true, std::sync::atomic::Ordering::SeqCst)
                }
            }),
            cell,
        },
    )
    .await
    .unwrap();
    Device {
        app,
        sync,
        events,
        changes,
        roles,
        driven,
        announce,
        _dir: dir,
    }
}

impl Device {
    fn request(&mut self, request: Request) -> Response {
        match self.app.handle(request) {
            Response::Error { message } => panic!("request failed: {message}"),
            r => r,
        }
    }

    /// What the app thread does: hands library changes to the app.
    fn pump(&mut self) {
        for presenting in std::mem::take(&mut *self.roles.lock().unwrap()) {
            self.app.set_presenting(presenting);
        }
        for request in std::mem::take(&mut *self.driven.lock().unwrap()) {
            if let Response::Error { message } = self.app.handle(request) {
                panic!("a driver's request failed: {message}");
            }
        }
        if self
            .announce
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            self.app.announce();
        }
        let batch: Vec<DocChanged> = self.changes.try_iter().collect();
        if !batch.is_empty() {
            self.app.library_changed(&batch);
        }
    }

    /// The cursor the last `Event::Live` this device emitted put on air.
    fn live_cursor(&self) -> Option<Cursor> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find_map(|e| match e {
                Event::Live { live, .. } => Some(*live),
                _ => None,
            })?
    }

    fn address(&self) -> String {
        format!("127.0.0.1:{}", self.sync.local_addr().port())
    }

    fn on_air(&self) -> String {
        self.app
            .live_text()
            .map(|t| t.lines.join(" / "))
            .unwrap_or_default()
    }

    fn last_loaded(&self) -> Option<LoadedService> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find_map(|e| match e {
                Event::Loaded(l) => Some(l.clone()),
                _ => None,
            })
    }
}

fn song_id(d: &mut Device, query: &str) -> SongId {
    match d.request(Request::Library(LibraryRequest::SearchSongs {
        query: query.into(),
        limit: 1,
    })) {
        Response::Songs(hits) => hits[0].song,
        other => panic!("{other:?}"),
    }
}

fn get_song(d: &mut Device, id: SongId) -> Song {
    match d.request(Request::Library(LibraryRequest::GetSong(id))) {
        Response::Song(s) => s,
        other => panic!("{other:?}"),
    }
}

async fn until(limit: Duration, what: &str, mut check: impl FnMut() -> bool) -> Duration {
    let start = Instant::now();
    while !check() {
        assert!(start.elapsed() < limit, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    start.elapsed()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_remote_edit_reloads_the_service_but_never_changes_what_is_on_air() {
    let mut presenter = device("Sanctuary").await;
    let mut office = device("Office PC").await;

    // Pair: the presenting machine shows a code, the office PC types it.
    let code = match presenter.sync.handle(TeamRequest::StartPairing).await {
        Response::Team(t) => t.pairing.unwrap().code,
        other => panic!("{other:?}"),
    };
    match office
        .sync
        .handle(TeamRequest::Join {
            invite: code,
            address: Some(presenter.address()),
        })
        .await
    {
        Response::Team(t) => assert_eq!(t.paired[0].name, "Sanctuary"),
        other => panic!("{other:?}"),
    }

    // The presenting machine plans and goes live on verse 1.
    let grace = song_id(&mut presenter, "Amazing Grace");
    let holy = song_id(&mut presenter, "Holy, Holy, Holy");
    let service = match presenter.request(Request::Service(ServiceRequest::Create {
        title: "Sunday".into(),
        starts_at: None,
        time_zone: None,
    })) {
        Response::Service(s) => s,
        other => panic!("{other:?}"),
    };
    for song in [grace, holy] {
        presenter.request(Request::Service(ServiceRequest::InsertItem {
            service: service.id,
            index: usize::MAX,
            kind: ItemKind::Song {
                song,
                arrangement: None,
            },
        }));
    }
    let loaded = match presenter.request(Request::Service(ServiceRequest::Open(service.id))) {
        Response::Loaded(l) => l,
        other => panic!("{other:?}"),
    };
    let first = loaded.items[0].id;
    let second = loaded.items[1].id;
    presenter.request(Request::Live(Command::Next));
    let verse1 = presenter.on_air();
    assert!(
        verse1.starts_with("Amazing grace! How sweet the sound"),
        "{verse1}"
    );

    // A device-only output on the office PC stays there.
    office.request(Request::Outputs(OutputRequest::Put(OutputConfig {
        id: OutputId::generate(),
        name: "Office preview".into(),
        role: OutputRole::Audience { primary: true },
        target: OutputTarget::Window {
            width: 640,
            height: 360,
        },
        safe_area: SafeArea::default(),
        background: BackgroundMode::Bleed,
        theme: None,
        idle: None,
        enabled: true,
    })));

    // The service reaches the office PC.
    until(
        Duration::from_secs(10),
        "the service on the office PC",
        || {
            office
                .app
                .library()
                .service(service.id)
                .unwrap()
                .is_some_and(|s: Service| s.items.len() == 2)
        },
    )
    .await;

    // Mid-service, the office PC fixes lyrics in both songs.
    let mut g = get_song(&mut office, grace);
    g.sections[0].lines[0] = "EDITED verse one".into();
    g.sections[1].lines[0] = "EDITED verse two".into();
    let mut h = get_song(&mut office, holy);
    h.sections[0].lines[0] = "EDITED holy".into();
    presenter.pump();
    presenter.events.lock().unwrap().clear();
    let start = Instant::now();
    office.request(Request::Library(LibraryRequest::PutSong(g)));
    office.request(Request::Library(LibraryRequest::PutSong(h)));

    // The presenting machine reloads the open service...
    let reloaded = |d: &Device| {
        d.last_loaded().is_some_and(|l| {
            l.items[1].slides[0].lines.join(" ").contains("EDITED holy")
                && l.items[0].slides[0]
                    .lines
                    .join(" ")
                    .contains("EDITED verse one")
        })
    };
    while !reloaded(&presenter) {
        assert!(start.elapsed() < Duration::from_secs(10), "no reload");
        presenter.pump();
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    let took = start.elapsed();
    println!("edit on the office PC → service reloaded on the presenting machine: {took:?}");
    assert!(took < Duration::from_secs(1), "{took:?}");

    // ...but what is on air does not change,
    assert_eq!(presenter.on_air(), verse1);
    // nor does the rest of the live song until the operator leaves it.
    presenter.request(Request::Live(Command::Next));
    let verse2 = presenter.on_air();
    assert!(verse2.starts_with("'Twas grace that taught"), "{verse2}");
    // The next song was not live, so it shows the fix at once.
    presenter.request(Request::Live(Command::Show(Cursor {
        item: second,
        slide: 0,
        step: 0,
    })));
    assert!(
        presenter.on_air().starts_with("EDITED holy"),
        "{}",
        presenter.on_air()
    );
    // Coming back to the first song, it has the fix too.
    presenter.request(Request::Live(Command::Show(Cursor {
        item: first,
        slide: 0,
        step: 0,
    })));
    assert!(presenter.on_air().starts_with("EDITED verse one"));

    // Output configuration never syncs.
    tokio::time::sleep(Duration::from_millis(200)).await;
    match presenter.request(Request::Outputs(OutputRequest::List)) {
        Response::Outputs(outputs) => {
            assert!(outputs.iter().all(|o| o.name != "Office preview"))
        }
        other => panic!("{other:?}"),
    }

    // The presenting machine was told it has a peer.
    assert!(
        presenter
            .events
            .lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, Event::SyncStatus { sync } if sync.connected == 1))
            || presenter.sync.status().paired.iter().any(|p| p.connected)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unpaired_device_gets_nothing() {
    let mut presenter = device("Sanctuary").await;
    let mut stranger = device("Stranger").await;
    let address = presenter.address();
    match stranger.sync.handle(TeamRequest::Connect { address }).await {
        Response::Error { message } => assert!(message.contains("not paired"), "{message}"),
        other => panic!("expected an error, got {other:?}"),
    }
    let mine = Song::new("Only on the presenting machine");
    presenter.request(Request::Library(LibraryRequest::PutSong(mine.clone())));
    tokio::time::sleep(Duration::from_millis(300)).await;
    stranger.pump();
    assert!(stranger.app.library().song(mine.id).unwrap().is_none());
    assert!(presenter.sync.status().paired.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn asking_to_present_moves_the_screens_from_one_app_to_the_other() {
    let mut sanctuary = device("Sanctuary").await;
    let mut laptop = device("Mary's laptop").await;
    let code = match sanctuary.sync.handle(TeamRequest::StartPairing).await {
        Response::Team(t) => t.pairing.unwrap().code,
        other => panic!("{other:?}"),
    };
    laptop
        .sync
        .handle(TeamRequest::Join {
            invite: code,
            address: Some(sanctuary.address()),
        })
        .await;
    let status = |d: &Device| d.sync.status().presenter;
    until(Duration::from_secs(5), "the roles to settle", || {
        status(&laptop).role == PresenterRole::Planning
            && status(&sanctuary).role == PresenterRole::Presenting
            && laptop.sync.status().paired.iter().any(|p| p.connected)
    })
    .await;
    laptop.pump();

    // The sanctuary is live; the laptop can plan but not present.
    let grace = song_id(&mut sanctuary, "Amazing Grace");
    let service = match sanctuary.request(Request::Service(ServiceRequest::Create {
        title: "Sunday".into(),
        starts_at: None,
        time_zone: None,
    })) {
        Response::Service(s) => s,
        other => panic!("{other:?}"),
    };
    sanctuary.request(Request::Service(ServiceRequest::InsertItem {
        service: service.id,
        index: usize::MAX,
        kind: ItemKind::Song {
            song: grace,
            arrangement: None,
        },
    }));
    sanctuary.request(Request::Service(ServiceRequest::Open(service.id)));
    sanctuary.request(Request::Live(Command::Next));
    assert!(!sanctuary.on_air().is_empty());
    until(Duration::from_secs(5), "the service on the laptop", || {
        laptop.pump();
        laptop.app.library().service(service.id).unwrap().is_some()
    })
    .await;
    assert!(matches!(
        laptop
            .app
            .handle(Request::Service(ServiceRequest::Open(service.id))),
        Response::Error { .. }
    ));

    // It asks; the sanctuary is shown the prompt and allows.
    assert!(matches!(
        laptop.sync.handle(TeamRequest::PresenterAsk).await,
        Response::Team(_)
    ));
    until(Duration::from_secs(5), "the prompt", || {
        !status(&sanctuary).requests.is_empty()
    })
    .await;
    let asker = sanctuary.sync.status().presenter.requests[0].device.clone();
    sanctuary
        .sync
        .handle(TeamRequest::PresenterAllow { device: asker })
        .await;
    until(Duration::from_secs(5), "the laptop to present", || {
        laptop.pump();
        sanctuary.pump();
        status(&laptop).role == PresenterRole::Presenting
    })
    .await;

    // The sanctuary is off air and refuses live control; the laptop opens
    // the service itself.
    until(Duration::from_secs(5), "the hand-over to clear it", || {
        sanctuary.pump();
        sanctuary.on_air().is_empty()
    })
    .await;
    assert!(matches!(
        sanctuary.app.handle(Request::Live(Command::Next)),
        Response::Error { .. }
    ));
    until(Duration::from_secs(5), "the laptop app to be told", || {
        laptop.pump();
        matches!(
            laptop
                .app
                .handle(Request::Service(ServiceRequest::Open(service.id))),
            Response::Loaded(_)
        )
    })
    .await;
    laptop.request(Request::Live(Command::Next));
    assert!(!laptop.on_air().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_driver_presses_next_and_the_presenter_moves_and_mirrors_back() {
    let mut sanctuary = device("Sanctuary").await;
    let mut laptop = device("Mary's laptop").await;
    let code = match sanctuary.sync.handle(TeamRequest::StartPairing).await {
        Response::Team(t) => t.pairing.unwrap().code,
        other => panic!("{other:?}"),
    };
    laptop
        .sync
        .handle(TeamRequest::Join {
            invite: code,
            address: Some(sanctuary.address()),
        })
        .await;
    let status = |d: &Device| d.sync.status().presenter;
    until(Duration::from_secs(5), "the roles to settle", || {
        status(&laptop).role == PresenterRole::Planning
            && status(&sanctuary).role == PresenterRole::Presenting
            && laptop.sync.status().paired.iter().any(|p| p.connected)
    })
    .await;
    laptop.pump();

    // The sanctuary has a service on air at verse 1.
    let grace = song_id(&mut sanctuary, "Amazing Grace");
    let service = match sanctuary.request(Request::Service(ServiceRequest::Create {
        title: "Sunday".into(),
        starts_at: None,
        time_zone: None,
    })) {
        Response::Service(s) => s,
        other => panic!("{other:?}"),
    };
    sanctuary.request(Request::Service(ServiceRequest::InsertItem {
        service: service.id,
        index: usize::MAX,
        kind: ItemKind::Song {
            song: grace,
            arrangement: None,
        },
    }));
    sanctuary.request(Request::Service(ServiceRequest::Open(service.id)));
    sanctuary.request(Request::Live(Command::Next));
    let verse1 = sanctuary.on_air();
    assert!(!verse1.is_empty());

    // Before it drives, the laptop's commands go nowhere: it only plans.
    assert!(laptop
        .sync
        .forward(&Request::Live(Command::Next))
        .await
        .is_none());
    until(Duration::from_secs(5), "the laptop app to be told", || {
        laptop.pump();
        matches!(
            laptop.app.handle(Request::Live(Command::Next)),
            Response::Error { .. }
        )
    })
    .await;

    // It asks to drive; the sanctuary shows the prompt and allows.
    assert!(matches!(
        laptop.sync.handle(TeamRequest::DriveAsk).await,
        Response::Team(_)
    ));
    until(Duration::from_secs(5), "the prompt", || {
        !status(&sanctuary).drive.requests.is_empty()
    })
    .await;
    let asker = status(&sanctuary).drive.requests[0].device.clone();
    sanctuary
        .sync
        .handle(TeamRequest::DriveAllow {
            device: asker,
            always: false,
        })
        .await;
    until(Duration::from_secs(5), "the laptop to drive", || {
        laptop.pump();
        sanctuary.pump();
        status(&laptop).drive.driving.is_some()
    })
    .await;
    // Presenting did not move.
    assert_eq!(status(&sanctuary).role, PresenterRole::Presenting);
    assert_eq!(status(&laptop).role, PresenterRole::Planning);

    // The laptop's Present screen is told what is on air and what is loaded.
    until(
        Duration::from_secs(5),
        "the laptop to mirror the sanctuary",
        || {
            sanctuary.pump();
            laptop.last_loaded().is_some() && laptop.live_cursor().is_some()
        },
    )
    .await;
    assert_eq!(laptop.live_cursor(), sanctuary.live_cursor());

    // Next on the laptop moves the sanctuary's screens, and the laptop's
    // screen follows.
    let sent = Instant::now();
    assert_eq!(
        laptop.sync.forward(&Request::Live(Command::Next)).await,
        Some(Response::Ok)
    );
    until(Duration::from_secs(5), "the sanctuary to move", || {
        sanctuary.pump();
        sanctuary.on_air() != verse1
    })
    .await;
    until(Duration::from_secs(5), "the laptop to follow", || {
        laptop.live_cursor() == sanctuary.live_cursor()
    })
    .await;
    eprintln!("Next crossed and came back in {:?}", sent.elapsed());
    assert!(sent.elapsed() < Duration::from_millis(500));
    // The laptop's own screens stay dark: it never put anything on air.
    assert!(laptop.on_air().is_empty());

    // Opening a service from the driver answers with its slides.
    sanctuary.request(Request::Service(ServiceRequest::Close(service.id)));
    let open = Request::Service(ServiceRequest::Open(service.id));
    let opened = laptop.sync.forward(&open);
    let pumping = async {
        loop {
            sanctuary.pump();
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    };
    let opened = tokio::select! {
        r = opened => r,
        _ = pumping => unreachable!(),
    };
    match opened {
        Some(Response::Loaded(l)) => assert_eq!(l.service, service.id),
        other => panic!("{other:?}"),
    }

    // The presenter's own keys always work, and it takes control back.
    sanctuary.request(Request::Live(Command::Next));
    sanctuary.sync.handle(TeamRequest::DriveTakeBack).await;
    until(Duration::from_secs(5), "control to come back", || {
        status(&laptop).drive.driving.is_none()
    })
    .await;
    assert!(laptop
        .sync
        .forward(&Request::Live(Command::Next))
        .await
        .is_none());
    let before = sanctuary.on_air();
    laptop.sync.forward(&Request::Live(Command::Next)).await;
    sanctuary.pump();
    assert_eq!(sanctuary.on_air(), before, "a former driver moves nothing");
}
