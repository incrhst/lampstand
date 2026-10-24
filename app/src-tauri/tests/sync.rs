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
    /// Times sync told the app to stop the music (it stopped being the Audio holder).
    stopped: Arc<std::sync::atomic::AtomicUsize>,
    _dir: tempfile::TempDir,
}

/// A device with a screen set up, as a church's machines have.
async fn device(name: &str) -> Device {
    device_with(name, true).await
}

/// A device with or without a screen. Without one it cannot hold the
/// Projection role (sync is told when the app has an output turned on).
async fn device_with(name: &str, screens: bool) -> Device {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let recorded = events.clone();
    let cell = NodeCell::default();
    let emit: lampstand_app::handler::EventSink = mirroring(
        Arc::new(move |e| recorded.lock().unwrap().push(e)),
        cell.clone(),
    );
    let mut app = App::open(
        dir.path(),
        Fonts::bundled_only(),
        Arc::new(Sink),
        Box::new(Host),
        emit.clone(),
    )
    .unwrap();
    if screens {
        app.handle(Request::Outputs(OutputRequest::Put(screen(name))));
    }
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
    // As the shell does once sync is running; it tells sync about the screens.
    app.set_sync_link(sync.link());
    let stopped = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = stopped.clone();
    sync.on_music_stop(Arc::new(move || {
        counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }));
    Device {
        app,
        sync,
        events,
        changes,
        roles,
        driven,
        announce,
        stopped,
        _dir: dir,
    }
}

fn screen(name: &str) -> OutputConfig {
    OutputConfig {
        id: OutputId::generate(),
        name: format!("{name} screen"),
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
            // The app is told of a role change on the next tick of the
            // sync watcher, a moment after the node itself has it. (It is
            // also told the role it starts with, so look at the latest.)
            && laptop.roles.lock().unwrap().last() == Some(&false)
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
            // The app is told of a role change on the next tick of the
            // sync watcher, a moment after the node itself has it. (It is
            // also told the role it starts with, so look at the latest.)
            && laptop.roles.lock().unwrap().last() == Some(&false)
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

#[tokio::test(flavor = "multi_thread")]
async fn a_driver_can_open_the_service_when_nothing_is_open_on_the_presenter() {
    // (See `learned_the_roles`: the laptop must know who holds Operator before
    // it asks, or it sees the seat as free and takes it.)
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
            && learned_the_roles(&sanctuary, &laptop)
    })
    .await;
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
    // The laptop may still be learning who presents when the roles settle (a
    // slow runner redials): an ask refused for that is asked again, as a
    // person would press the button again.
    let start = Instant::now();
    while let Response::Error { .. } = laptop.sync.handle(TeamRequest::DriveAsk).await {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "the ask was refused"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
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
    // The service has to reach the laptop's library only for its list; the
    // presenter opens it from its own.
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
    assert!(matches!(opened, Some(Response::Loaded(_))), "{opened:?}");
    // Then Next from the driver.
    laptop.sync.forward(&Request::Live(Command::Next)).await;
    until(
        Duration::from_secs(5),
        "the presenter to show verse 1",
        || {
            sanctuary.pump();
            !sanctuary.on_air().is_empty()
        },
    )
    .await;
}

// ---- Roles --------------------------------------------------------------------

use lampstand_api::{PresenterStanding, RoleName, RoleStanding, RoleView};
use lampstand_sync::roles::Role;

/// The joining device has heard who holds Projection, Operator and Audio (the
/// device that showed the code). Until it has, those roles look free to it and
/// a press of "Take" or "Ask" would take one instead of asking: a short window
/// after pairing that tests must wait out (a known limit from PR #142).
fn learned_the_roles(main: &Device, other: &Device) -> bool {
    let r = other.sync.node().roles_status();
    let held = Some(main.sync.node().device());
    r.projection.record.holder == held
        && r.operator.record.holder == held
        && r.audio.record.holder == held
}

/// Pairs `other` with `main` (which shows the code) and waits until both
/// have the roles pairing leaves: the main machine holds Projection,
/// Operator and Audio.
async fn pair_up(main: &Device, other: &Device) {
    let code = match main.sync.handle(TeamRequest::StartPairing).await {
        Response::Team(t) => t.pairing.unwrap().code,
        other => panic!("{other:?}"),
    };
    other
        .sync
        .handle(TeamRequest::Join {
            invite: code,
            address: Some(main.address()),
        })
        .await;
    until(Duration::from_secs(5), "the roles to settle", || {
        let held = |d: &Device| d.sync.node().roles_status().audio.record.holder;
        held(other) == Some(main.sync.node().device())
            && learned_the_roles(main, other)
            && main.sync.node().status().connected() == 1
            && other.sync.node().status().connected() == 1
    })
    .await;
}

fn team(d: &Device) -> lampstand_api::TeamStatus {
    d.sync.status()
}

/// A church with one device and nothing paired sees nothing new: the same
/// view the screens read in 0.7.0, no prompt, no button, no wording, and
/// nothing written for roles. (The API and the web app are untouched by the
/// role record, so there is no new text to show.) It holds all four roles
/// even with no screen set up.
#[tokio::test(flavor = "multi_thread")]
async fn a_church_with_one_device_and_no_pairing_sees_nothing_new() {
    let mut lone = device_with("Sanctuary", false).await;
    let t = team(&lone);
    assert!(t.paired.is_empty() && t.nearby.is_empty() && t.pairing.is_none());
    assert_eq!(t.sync.state, lampstand_api::SyncState::DeviceOnly);
    let p = &t.presenter;
    assert_eq!(p.role, PresenterRole::Presenting);
    assert_eq!(p.standing, PresenterStanding::Presenting);
    assert!(p.requests.is_empty() && p.asking.is_none() && p.replaced_by.is_none());
    assert_eq!(p.takeover_secs, 120);
    let d = &*p.drive;
    assert!(d.driving.is_none() && d.active.is_none() && d.requests.is_empty());
    assert!(d.asking.is_none() && !d.can_drive && d.always.is_empty());
    assert!(d.taken_back_by.is_none());

    // The four roles are reported, all held here, with nothing to answer,
    // nothing to notice and nothing greyed out.
    assert_eq!(t.roles.len(), 4);
    for r in &t.roles {
        assert!(r.holds, "{:?}", r.role);
        assert_eq!(r.standing, RoleStanding::Holding);
        assert!(r.request.is_none() && r.asking.is_none() && r.replaced_by.is_none());
        assert!(r.blocked.is_none() && r.always.is_empty());
    }
    assert_eq!(lone.stopped.load(std::sync::atomic::Ordering::SeqCst), 0);

    // Nothing is written for roles until something is claimed.
    assert!(!lone._dir.path().join("sync/roles.json").exists());

    // Asking to hand over says what it said before: the device already has
    // the job. (Claiming on a lone device records the claim, as it did.)
    match lone.sync.handle(TeamRequest::PresenterAsk).await {
        Response::Error { message } => {
            assert_eq!(message, "This device already runs the screens.")
        }
        other => panic!("{other:?}"),
    }
    match lone.sync.handle(TeamRequest::DriveAsk).await {
        Response::Error { message } => {
            assert_eq!(message, "This device already runs the service.")
        }
        other => panic!("{other:?}"),
    }

    // It can take a service live, with no screen at all.
    let grace = song_id(&mut lone, "Amazing Grace");
    let service = match lone.request(Request::Service(ServiceRequest::Create {
        title: "Sunday".into(),
        starts_at: None,
        time_zone: None,
    })) {
        Response::Service(s) => s,
        other => panic!("{other:?}"),
    };
    lone.request(Request::Service(ServiceRequest::InsertItem {
        service: service.id,
        index: usize::MAX,
        kind: ItemKind::Song {
            song: grace,
            arrangement: None,
        },
    }));
    lone.request(Request::Service(ServiceRequest::Open(service.id)));
    lone.request(Request::Live(Command::Next));
    assert!(!lone.on_air().is_empty());
    // The app was only ever told it presents.
    assert!(lone.roles.lock().unwrap().iter().all(|p| *p));
    let r = lone.sync.node().roles_status();
    assert!(r.projection.holds && r.operator.holds && r.stream.holds && r.audio.holds);
}

/// Projection can be held only by a device with screens set up, and the app
/// tells sync when that changes.
#[tokio::test(flavor = "multi_thread")]
async fn projection_needs_a_screen_and_the_app_tells_sync_when_one_is_set_up() {
    let main = device("Sanctuary").await;
    let mut laptop = device_with("Mary's laptop", false).await;
    pair_up(&main, &laptop).await;

    for request in [TeamRequest::PresenterAsk, TeamRequest::PresenterClaim] {
        match laptop.sync.handle(request).await {
            Response::Error { message } => {
                assert_eq!(message, "No screens are set up on this device")
            }
            other => panic!("{other:?}"),
        }
    }
    assert!(main.sync.status().presenter.requests.is_empty());
    // The other roles do not need a screen: it can ask to run the sound.
    laptop.sync.node().ask_role(Role::Audio).unwrap();

    // Turning on an output (Setup > Screens) is what lets it ask.
    laptop.request(Request::Outputs(OutputRequest::Put(screen(
        "Mary's laptop",
    ))));
    assert!(matches!(
        laptop.sync.handle(TeamRequest::PresenterAsk).await,
        Response::Team(_)
    ));
    until(Duration::from_secs(5), "the prompt", || {
        !main.sync.status().presenter.requests.is_empty()
    })
    .await;
    // And removing it again closes the way (the holder keeps what it has).
    let outputs = match laptop.request(Request::Outputs(OutputRequest::List)) {
        Response::Outputs(o) => o,
        other => panic!("{other:?}"),
    };
    for o in outputs {
        laptop.request(Request::Outputs(OutputRequest::Remove(o.id)));
    }
    laptop.sync.handle(TeamRequest::PresenterCancel).await;
    match laptop.sync.handle(TeamRequest::PresenterAsk).await {
        Response::Error { message } => {
            assert_eq!(message, "No screens are set up on this device")
        }
        other => panic!("{other:?}"),
    }
}

/// Only passing Projection clears the screens. Passing Operator, Stream or
/// Audio leaves the slide up. (Audio's own effect, stopping the music, comes
/// with the sound itself; the rule is `Role::on_air`.)
#[tokio::test(flavor = "multi_thread")]
async fn only_passing_projection_clears_the_screens() {
    let mut sanctuary = device("Sanctuary").await;
    let mut laptop = device("Mary's laptop").await;
    pair_up(&sanctuary, &laptop).await;
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
    let on_air = sanctuary.on_air();
    assert!(!on_air.is_empty());

    for role in [Role::Operator, Role::Audio] {
        laptop.sync.node().ask_role(role).unwrap();
        until(Duration::from_secs(5), "the prompt", || {
            let r = sanctuary.sync.node().roles_status();
            match role {
                Role::Operator => r.operator.request.is_some(),
                _ => r.audio.request.is_some(),
            }
        })
        .await;
        sanctuary
            .sync
            .node()
            .allow_role(role, laptop.sync.node().device(), false)
            .unwrap();
        sanctuary.pump();
        assert_eq!(sanctuary.on_air(), on_air, "{role:?} leaves the slide up");
    }
    // The sanctuary still presents; the laptop runs the service and the sound.
    assert_eq!(team(&sanctuary).presenter.role, PresenterRole::Presenting);
    until(Duration::from_secs(5), "the laptop to drive", || {
        laptop.pump();
        sanctuary.pump();
        team(&laptop).presenter.drive.driving.is_some()
    })
    .await;
    assert_eq!(sanctuary.on_air(), on_air);

    // Stream is unheld; taking it changes nothing on air either.
    laptop.sync.node().claim_role(Role::Stream).unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    sanctuary.pump();
    assert_eq!(sanctuary.on_air(), on_air);

    // Projection is different: the screens go clear.
    laptop.sync.handle(TeamRequest::PresenterAsk).await;
    until(Duration::from_secs(5), "the prompt", || {
        !team(&sanctuary).presenter.requests.is_empty()
    })
    .await;
    let asker = team(&sanctuary).presenter.requests[0].device.clone();
    sanctuary
        .sync
        .handle(TeamRequest::PresenterAllow { device: asker })
        .await;
    until(Duration::from_secs(5), "the screens to clear", || {
        sanctuary.pump();
        sanctuary.on_air().is_empty()
    })
    .await;
    // The Operator seat is kept across the hand-over: the sanctuary no
    // longer holds it, the laptop does, and it now holds Projection too.
    let roles = sanctuary.sync.node().roles_status();
    assert_eq!(
        roles.operator.record.holder,
        Some(laptop.sync.node().device())
    );
    assert_eq!(
        roles.projection.record.holder,
        Some(laptop.sync.node().device())
    );
}

// ---- Asking, always-allow and takeover for every role --------------------------

fn role(d: &Device, role: RoleName) -> RoleView {
    team(d).roles.into_iter().find(|r| r.role == role).unwrap()
}

async fn send(d: &Device, request: TeamRequest) -> Response {
    d.sync.handle(request).await
}

async fn send_ok(d: &Device, request: TeamRequest) {
    let what = format!("{request:?}");
    match send(d, request).await {
        Response::Team(_) => {}
        other => panic!("{what} on {}: {other:?}", d.sync.status().name),
    }
}

async fn send_err(d: &Device, request: TeamRequest) -> String {
    match send(d, request).await {
        Response::Error { message } => message,
        other => panic!("expected an error, got {other:?}"),
    }
}

fn key(d: &Device) -> String {
    d.sync.node().device().to_hex()
}

/// Pairs a further device with `main`, which shows a code again.
async fn pair_more(main: &Device, other: &Device, connected: usize) {
    let code = match send(main, TeamRequest::StartPairing).await {
        Response::Team(t) => t.pairing.unwrap().code,
        other => panic!("{other:?}"),
    };
    send(
        other,
        TeamRequest::Join {
            invite: code,
            address: Some(main.address()),
        },
    )
    .await;
    until(Duration::from_secs(20), "the new device to connect", || {
        main.sync.node().status().connected() == connected
            && other.sync.node().status().connected() == 1
    })
    .await;
}

/// Something on air on `d`, for the on-air rules.
fn put_on_air(d: &mut Device) -> String {
    let grace = song_id(d, "Amazing Grace");
    let service = match d.request(Request::Service(ServiceRequest::Create {
        title: "Sunday".into(),
        starts_at: None,
        time_zone: None,
    })) {
        Response::Service(s) => s,
        other => panic!("{other:?}"),
    };
    d.request(Request::Service(ServiceRequest::InsertItem {
        service: service.id,
        index: usize::MAX,
        kind: ItemKind::Song {
            song: grace,
            arrangement: None,
        },
    }));
    d.request(Request::Service(ServiceRequest::Open(service.id)));
    d.request(Request::Live(Command::Next));
    let on_air = d.on_air();
    assert!(!on_air.is_empty());
    on_air
}

/// Ask, "not now", ask again, allow: for each role, through the app's
/// requests. "Always allow" sticks for Operator and Stream only.
#[tokio::test(flavor = "multi_thread")]
async fn every_role_can_be_asked_for_declined_and_allowed() {
    let main = device("Sanctuary").await;
    let laptop = device("Mary's laptop").await;
    pair_up(&main, &laptop).await;
    // Pairing leaves Stream unheld; the main machine takes it.
    send_ok(
        &main,
        TeamRequest::RoleClaim {
            role: RoleName::Stream,
        },
    )
    .await;
    until(Duration::from_secs(20), "the laptop to hear", || {
        role(&laptop, RoleName::Stream).standing == RoleStanding::Ask
    })
    .await;

    for name in [
        RoleName::Stream,
        RoleName::Operator,
        RoleName::Audio,
        RoleName::Projection,
    ] {
        let always_offered = matches!(name, RoleName::Stream | RoleName::Operator);
        assert_eq!(role(&main, name).offers_always, always_offered);
        assert!(role(&main, name).holds, "{name:?}");
        assert_eq!(role(&laptop, name).standing, RoleStanding::Ask);

        send_ok(&laptop, TeamRequest::RoleAsk { role: name }).await;
        until(Duration::from_secs(20), "the prompt", || {
            role(&main, name).request.is_some()
        })
        .await;
        let asker = role(&main, name).request.unwrap();
        assert_eq!(asker.name, "Mary's laptop");
        assert_eq!(asker.device, key(&laptop));

        // Not now.
        send_ok(
            &main,
            TeamRequest::RoleDecline {
                role: name,
                device: asker.device.clone(),
            },
        )
        .await;
        until(Duration::from_secs(20), "the answer", || {
            role(&laptop, name).asking == Some(lampstand_api::RoleAskingView::Declined)
        })
        .await;
        assert!(role(&main, name).request.is_none());
        send_ok(&laptop, TeamRequest::RoleCancel { role: name }).await;
        assert!(role(&laptop, name).asking.is_none());

        // Ask again; this time allowed (with "always" where it is offered).
        send_ok(&laptop, TeamRequest::RoleAsk { role: name }).await;
        until(Duration::from_secs(20), "the second prompt", || {
            role(&main, name).request.is_some()
        })
        .await;
        send_ok(
            &main,
            TeamRequest::RoleAllow {
                role: name,
                device: asker.device.clone(),
                always: true,
            },
        )
        .await;
        until(Duration::from_secs(20), "the laptop to hold it", || {
            role(&laptop, name).holds && !role(&main, name).holds
        })
        .await;
        assert_eq!(
            role(&main, name).always.len(),
            usize::from(always_offered),
            "{name:?}: always-allow is remembered for Operator and Stream only"
        );
        // It gave the role up on purpose: no notice.
        assert!(role(&main, name).replaced_by.is_none());
        if name == RoleName::Projection {
            continue;
        }
        give_back(&main, &laptop, name).await;
        // A remembered device is let in without a prompt.
        if always_offered {
            send_ok(&laptop, TeamRequest::RoleAsk { role: name }).await;
            until(Duration::from_secs(20), "the remembered device in", || {
                role(&laptop, name).holds
            })
            .await;
            assert!(role(&main, name).request.is_none());
            // Forget it; the next ask prompts again.
            send_ok(
                &main,
                TeamRequest::RoleAlways {
                    role: name,
                    device: key(&laptop),
                    always: false,
                },
            )
            .await;
            assert!(role(&main, name).always.is_empty());
            give_back(&main, &laptop, name).await;
            send_ok(&laptop, TeamRequest::RoleAsk { role: name }).await;
            until(Duration::from_secs(20), "a prompt again", || {
                role(&main, name).request.is_some()
            })
            .await;
            send_ok(
                &main,
                TeamRequest::RoleDecline {
                    role: name,
                    device: key(&laptop),
                },
            )
            .await;
            send_ok(&laptop, TeamRequest::RoleCancel { role: name }).await;
        }
    }
}

/// The main machine gets a role back from the laptop: the Projection holder
/// takes the Operator seat with no ask; the rest are asked for.
async fn give_back(main: &Device, laptop: &Device, name: RoleName) {
    if name == RoleName::Operator {
        send_ok(main, TeamRequest::RoleClaim { role: name }).await;
        until(Duration::from_secs(20), "the laptop to hear", || {
            !role(laptop, name).holds
        })
        .await;
        // The laptop was told, as a driver is told it was taken back.
        send_ok(laptop, TeamRequest::RoleDismiss { role: name }).await;
        return;
    }
    send_ok(main, TeamRequest::RoleAsk { role: name }).await;
    until(Duration::from_secs(20), "the laptop's prompt", || {
        role(laptop, name).request.is_some()
    })
    .await;
    send_ok(
        laptop,
        TeamRequest::RoleAllow {
            role: name,
            device: key(main),
            always: false,
        },
    )
    .await;
    until(
        Duration::from_secs(20),
        "the main machine to hold it",
        || role(main, name).holds && !role(laptop, name).holds,
    )
    .await;
}

/// Three devices: a second asker is told who is asking and is not queued.
#[tokio::test(flavor = "multi_thread")]
async fn a_second_asker_is_told_who_is_already_asking() {
    let main = device("Sanctuary").await;
    let laptop = device("Mary's laptop").await;
    let tablet = device("Sam's tablet").await;
    pair_up(&main, &laptop).await;
    pair_more(&main, &tablet, 2).await;
    send_ok(
        &main,
        TeamRequest::RoleClaim {
            role: RoleName::Stream,
        },
    )
    .await;
    until(Duration::from_secs(20), "both to hear", || {
        role(&laptop, RoleName::Stream).standing == RoleStanding::Ask
            && role(&tablet, RoleName::Stream).standing == RoleStanding::Ask
    })
    .await;

    send_ok(
        &laptop,
        TeamRequest::RoleAsk {
            role: RoleName::Stream,
        },
    )
    .await;
    until(Duration::from_secs(20), "the first prompt", || {
        role(&main, RoleName::Stream).request.is_some()
    })
    .await;
    send_ok(
        &tablet,
        TeamRequest::RoleAsk {
            role: RoleName::Stream,
        },
    )
    .await;
    until(Duration::from_secs(20), "the tablet to be told", || {
        role(&tablet, RoleName::Stream).asking
            == Some(lampstand_api::RoleAskingView::Busy {
                by: "Mary's laptop".into(),
            })
    })
    .await;
    // Still the laptop's prompt, one at a time.
    assert_eq!(
        role(&main, RoleName::Stream).request.unwrap().name,
        "Mary's laptop"
    );

    // The laptop is allowed; the tablet asks again and now asks the laptop.
    send_ok(
        &main,
        TeamRequest::RoleAllow {
            role: RoleName::Stream,
            device: key(&laptop),
            always: false,
        },
    )
    .await;
    until(
        Duration::from_secs(20),
        "the laptop to hold the stream",
        || role(&laptop, RoleName::Stream).holds,
    )
    .await;
    send_ok(
        &tablet,
        TeamRequest::RoleCancel {
            role: RoleName::Stream,
        },
    )
    .await;
    // Pairing is not transitive: the tablet only knows the main machine, so
    // it reads the stream as the main machine's record says (a known limit,
    // PR #142). What matters here: the main machine has no prompt left.
    assert!(role(&main, RoleName::Stream).request.is_none());
}

/// A holder that is cut off for the takeover time can be taken from after
/// confirming (the claim), not before; the returning holder yields, with a
/// notice, and Audio's music stops on it.
#[tokio::test(flavor = "multi_thread")]
async fn takeover_after_a_cut_and_the_returning_holder_yields() {
    cut_takeover_and_return(false).await;
}

/// The same, with both devices dialling each other the moment the network
/// is back (as their redialling can), so two connections race and one is
/// dropped for the other. The returning holder must still yield.
#[tokio::test(flavor = "multi_thread")]
async fn the_returning_holder_yields_when_both_devices_dial_at_once() {
    cut_takeover_and_return(true).await;
}

async fn cut_takeover_and_return(both_dial: bool) {
    let main = device("Sanctuary").await;
    let laptop = device("Mary's laptop").await;
    pair_up(&main, &laptop).await;
    send_ok(
        &main,
        TeamRequest::RoleClaim {
            role: RoleName::Stream,
        },
    )
    .await;
    until(Duration::from_secs(20), "the laptop to hear", || {
        role(&laptop, RoleName::Stream).standing == RoleStanding::Ask
    })
    .await;
    // A claim against a device is judged by that device's own takeover time.
    send_ok(&main, TeamRequest::SetTakeover { seconds: 2 }).await;
    send_ok(&laptop, TeamRequest::SetTakeover { seconds: 2 }).await;

    // Cut both ways, as a dead network would.
    main.sync.cut();
    laptop.sync.cut();
    until(Duration::from_secs(20), "the laptop to see it", || {
        laptop.sync.node().status().connected() == 0
    })
    .await;
    // Not yet: and with the holder away there is nobody to ask.
    let soon = send_err(
        &laptop,
        TeamRequest::RoleClaim {
            role: RoleName::Stream,
        },
    )
    .await;
    assert!(soon.contains("went offline only recently"), "{soon}");
    assert_eq!(role(&laptop, RoleName::Stream).standing, RoleStanding::Wait);
    let nobody = send_err(
        &laptop,
        TeamRequest::RoleAsk {
            role: RoleName::Stream,
        },
    )
    .await;
    assert_eq!(nobody, "Sanctuary is not connected, so it cannot be asked.");

    until(Duration::from_secs(15), "takeover to open", || {
        role(&laptop, RoleName::Stream).standing == RoleStanding::TakeOver
            && role(&laptop, RoleName::Audio).standing == RoleStanding::TakeOver
    })
    .await;
    assert!(role(&laptop, RoleName::Stream).offline_secs.unwrap() >= 2);
    send_ok(
        &laptop,
        TeamRequest::RoleClaim {
            role: RoleName::Stream,
        },
    )
    .await;
    send_ok(
        &laptop,
        TeamRequest::RoleClaim {
            role: RoleName::Audio,
        },
    )
    .await;
    assert!(role(&laptop, RoleName::Stream).holds && role(&laptop, RoleName::Audio).holds);
    // The main machine still believes it runs both.
    assert!(role(&main, RoleName::Stream).holds);
    assert_eq!(main.stopped.load(std::sync::atomic::Ordering::SeqCst), 0);

    main.sync.restore().await.unwrap();
    laptop.sync.restore().await.unwrap();
    if both_dial {
        let (to_main, to_laptop) = (main.address(), laptop.address());
        tokio::join!(
            send(&laptop, TeamRequest::Connect { address: to_main }),
            send(&main, TeamRequest::Connect { address: to_laptop }),
        );
    }
    until(Duration::from_secs(60), "the old holder to yield", || {
        !role(&main, RoleName::Stream).holds && !role(&main, RoleName::Audio).holds
    })
    .await;
    assert_eq!(
        role(&main, RoleName::Stream).replaced_by.as_deref(),
        Some("Mary's laptop")
    );
    assert_eq!(
        role(&main, RoleName::Audio).replaced_by.as_deref(),
        Some("Mary's laptop")
    );
    // Roles are independent: it still runs the screens and the service.
    assert!(role(&main, RoleName::Projection).holds && role(&main, RoleName::Operator).holds);
    assert!(role(&main, RoleName::Projection).replaced_by.is_none());
    // Audio was playing here: it stops.
    until(Duration::from_secs(10), "the music to stop", || {
        main.stopped.load(std::sync::atomic::Ordering::SeqCst) == 1
    })
    .await;
    // It never resumes by itself, and the notice goes once dismissed.
    send_ok(
        &main,
        TeamRequest::RoleDismiss {
            role: RoleName::Stream,
        },
    )
    .await;
    assert!(role(&main, RoleName::Stream).replaced_by.is_none());
    assert!(role(&laptop, RoleName::Stream).holds);
}

/// What a hand-over takes off air: the screens for Projection, the music
/// for Audio; Operator and Stream leave everything as it is.
#[tokio::test(flavor = "multi_thread")]
async fn on_air_rules_follow_the_role_handed_over() {
    let mut main = device("Sanctuary").await;
    let laptop = device("Mary's laptop").await;
    pair_up(&main, &laptop).await;
    send_ok(
        &main,
        TeamRequest::RoleClaim {
            role: RoleName::Stream,
        },
    )
    .await;
    // The laptop must hear it before it asks, or Stream looks free to it.
    until(Duration::from_secs(20), "the laptop to hear", || {
        role(&laptop, RoleName::Stream).standing == RoleStanding::Ask
    })
    .await;
    let on_air = put_on_air(&mut main);

    for name in [RoleName::Stream, RoleName::Operator] {
        send_ok(&laptop, TeamRequest::RoleAsk { role: name }).await;
        until(Duration::from_secs(20), "the prompt", || {
            role(&main, name).request.is_some()
        })
        .await;
        send_ok(
            &main,
            TeamRequest::RoleAllow {
                role: name,
                device: key(&laptop),
                always: false,
            },
        )
        .await;
        // The hand-over has happened on both devices; only then is "nothing
        // changed on air" worth asserting.
        until(Duration::from_secs(20), "the hand-over to settle", || {
            role(&laptop, name).holds && !role(&main, name).holds
        })
        .await;
        main.pump();
        assert_eq!(main.on_air(), on_air, "{name:?} leaves the slide up");
        assert_eq!(
            main.stopped.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "{name:?} leaves the music"
        );
    }

    send_ok(
        &laptop,
        TeamRequest::RoleAsk {
            role: RoleName::Audio,
        },
    )
    .await;
    until(Duration::from_secs(20), "the prompt", || {
        role(&main, RoleName::Audio).request.is_some()
    })
    .await;
    send_ok(
        &main,
        TeamRequest::RoleAllow {
            role: RoleName::Audio,
            device: key(&laptop),
            always: false,
        },
    )
    .await;
    until(Duration::from_secs(20), "the music to stop", || {
        main.stopped.load(std::sync::atomic::Ordering::SeqCst) == 1
    })
    .await;
    main.pump();
    assert_eq!(
        main.on_air(),
        on_air,
        "stopping the music leaves the slide up"
    );

    send_ok(
        &laptop,
        TeamRequest::RoleAsk {
            role: RoleName::Projection,
        },
    )
    .await;
    until(Duration::from_secs(20), "the prompt", || {
        role(&main, RoleName::Projection).request.is_some()
    })
    .await;
    send_ok(
        &main,
        TeamRequest::RoleAllow {
            role: RoleName::Projection,
            device: key(&laptop),
            always: false,
        },
    )
    .await;
    until(Duration::from_secs(20), "the screens to clear", || {
        main.pump();
        main.on_air().is_empty()
    })
    .await;
    assert_eq!(main.stopped.load(std::sync::atomic::Ordering::SeqCst), 1);
}

/// The Operator seat is kept when Projection moves, the new Projection
/// holder announces itself to the Operator, the Projection holder may take
/// the seat back with no ask, and "Stop driving" hands it back with no notice.
#[tokio::test(flavor = "multi_thread")]
async fn the_operator_seat_survives_a_projection_hand_over_and_comes_back_without_asking() {
    let main = device("Sanctuary").await;
    let laptop = device("Mary's laptop").await;
    pair_up(&main, &laptop).await;
    // The laptop runs the service from its own screen (a remembered ask).
    send_ok(
        &laptop,
        TeamRequest::RoleAsk {
            role: RoleName::Operator,
        },
    )
    .await;
    until(Duration::from_secs(20), "the prompt", || {
        role(&main, RoleName::Operator).request.is_some()
    })
    .await;
    send_ok(
        &main,
        TeamRequest::RoleAllow {
            role: RoleName::Operator,
            device: key(&laptop),
            always: true,
        },
    )
    .await;
    until(
        Duration::from_secs(20),
        "the laptop to run the service",
        || role(&laptop, RoleName::Operator).holds,
    )
    .await;

    // Projection moves to the laptop... which asks the other way round: give
    // the main machine the Operator seat back is not needed; the seat is
    // kept by whoever holds it.
    send_ok(
        &laptop,
        TeamRequest::RoleAsk {
            role: RoleName::Projection,
        },
    )
    .await;
    until(Duration::from_secs(20), "the prompt", || {
        role(&main, RoleName::Projection).request.is_some()
    })
    .await;
    main.announce
        .store(false, std::sync::atomic::Ordering::SeqCst);
    send_ok(
        &main,
        TeamRequest::RoleAllow {
            role: RoleName::Projection,
            device: key(&laptop),
            always: false,
        },
    )
    .await;
    until(Duration::from_secs(20), "projection to move", || {
        role(&laptop, RoleName::Projection).holds
    })
    .await;
    // The Operator seat did not move with it.
    assert!(role(&laptop, RoleName::Operator).holds);

    // Now the other way: the main machine asks for the screens back, and
    // the laptop, which runs the service, still does after the hand-over.
    send_ok(
        &main,
        TeamRequest::RoleAsk {
            role: RoleName::Projection,
        },
    )
    .await;
    until(Duration::from_secs(20), "the laptop's prompt", || {
        role(&laptop, RoleName::Projection).request.is_some()
    })
    .await;
    send_ok(
        &laptop,
        TeamRequest::RoleAllow {
            role: RoleName::Projection,
            device: key(&main),
            always: false,
        },
    )
    .await;
    until(Duration::from_secs(20), "projection to come back", || {
        role(&main, RoleName::Projection).holds
    })
    .await;
    assert!(role(&laptop, RoleName::Operator).holds, "the seat is kept");
    assert!(team(&laptop).presenter.drive.driving.is_some());
    // The new Projection holder announces its state to the Operator.
    until(Duration::from_secs(20), "the announcement", || {
        main.announce.load(std::sync::atomic::Ordering::SeqCst)
    })
    .await;

    // The Projection holder takes the seat back with no ask.
    send_ok(
        &main,
        TeamRequest::RoleClaim {
            role: RoleName::Operator,
        },
    )
    .await;
    assert!(role(&main, RoleName::Operator).holds);
    until(Duration::from_secs(20), "the laptop to hear", || {
        !role(&laptop, RoleName::Operator).holds
    })
    .await;
    // The device that was running the service is told it was taken back,
    // once (the driver's "took control back").
    assert_eq!(
        role(&laptop, RoleName::Operator).replaced_by.as_deref(),
        Some("Sanctuary")
    );
    send_ok(
        &laptop,
        TeamRequest::RoleDismiss {
            role: RoleName::Operator,
        },
    )
    .await;

    // "Stop driving" hands the seat back with no notice on the stopper.
    send_ok(
        &laptop,
        TeamRequest::RoleAsk {
            role: RoleName::Operator,
        },
    )
    .await;
    until(
        Duration::from_secs(20),
        "the laptop to run the service again",
        || role(&laptop, RoleName::Operator).holds,
    )
    .await;
    send_ok(&laptop, TeamRequest::DriveCancel).await;
    until(Duration::from_secs(20), "the seat to go back", || {
        role(&main, RoleName::Operator).holds && !role(&laptop, RoleName::Operator).holds
    })
    .await;
    assert!(role(&laptop, RoleName::Operator).replaced_by.is_none());
}

/// The words a refusal uses name the job, never "the presenting device".
#[tokio::test(flavor = "multi_thread")]
async fn refusals_use_the_roles_words() {
    let main = device("Sanctuary").await;
    let laptop = device_with("Mary's laptop", false).await;
    pair_up(&main, &laptop).await;
    send_ok(
        &main,
        TeamRequest::RoleClaim {
            role: RoleName::Stream,
        },
    )
    .await;
    until(Duration::from_secs(20), "the laptop to hear", || {
        role(&laptop, RoleName::Stream).standing == RoleStanding::Ask
    })
    .await;
    // Taking what is held by a connected device needs an ask.
    let must = send_err(
        &laptop,
        TeamRequest::RoleClaim {
            role: RoleName::Stream,
        },
    )
    .await;
    assert_eq!(must, "Sanctuary is connected: ask it to allow this.");
    assert_eq!(
        role(&laptop, RoleName::Projection).blocked,
        Some(lampstand_api::RoleBlocked::NoScreens)
    );
    let none = send_err(
        &laptop,
        TeamRequest::RoleAsk {
            role: RoleName::Projection,
        },
    )
    .await;
    assert_eq!(none, "No screens are set up on this device");
    // The holder cannot hand over what it does not hold, or to a device that
    // is not asking.
    let not_asking = send_err(
        &main,
        TeamRequest::RoleAllow {
            role: RoleName::Stream,
            device: key(&laptop),
            always: false,
        },
    )
    .await;
    assert_eq!(not_asking, "That device is no longer asking.");
    let not_holding = send_err(
        &laptop,
        TeamRequest::RoleAllow {
            role: RoleName::Audio,
            device: key(&main),
            always: false,
        },
    )
    .await;
    assert_eq!(not_holding, "This device does not run the sound.");
    let mine = send_err(
        &main,
        TeamRequest::RoleAsk {
            role: RoleName::Stream,
        },
    )
    .await;
    assert_eq!(mine, "This device already runs the stream.");
}

/// An ask nobody answered, and then the holder drops off: no answer can come,
/// so the asker can take over once the time is up. The holder's prompt goes
/// when the asker disconnects.
#[tokio::test(flavor = "multi_thread")]
async fn an_unanswered_ask_gives_way_to_takeover_when_the_holder_goes_offline() {
    let main = device("Sanctuary").await;
    let laptop = device("Mary's laptop").await;
    pair_up(&main, &laptop).await;
    send_ok(
        &main,
        TeamRequest::RoleClaim {
            role: RoleName::Stream,
        },
    )
    .await;
    until(Duration::from_secs(20), "the laptop to hear", || {
        role(&laptop, RoleName::Stream).standing == RoleStanding::Ask
    })
    .await;
    send_ok(&main, TeamRequest::SetTakeover { seconds: 2 }).await;
    send_ok(&laptop, TeamRequest::SetTakeover { seconds: 2 }).await;
    send_ok(
        &laptop,
        TeamRequest::RoleAsk {
            role: RoleName::Stream,
        },
    )
    .await;
    until(Duration::from_secs(20), "the prompt", || {
        role(&main, RoleName::Stream).request.is_some()
    })
    .await;
    assert_eq!(
        role(&laptop, RoleName::Stream).asking,
        Some(lampstand_api::RoleAskingView::Waiting)
    );

    main.sync.cut();
    laptop.sync.cut();
    // The holder's prompt goes when the asker disconnects.
    until(Duration::from_secs(20), "the prompt to go", || {
        role(&main, RoleName::Stream).request.is_none()
    })
    .await;
    // The asker is still "waiting", but the standing now says what to do.
    until(Duration::from_secs(20), "takeover to open", || {
        role(&laptop, RoleName::Stream).standing == RoleStanding::TakeOver
    })
    .await;
    assert_eq!(
        role(&laptop, RoleName::Stream).asking,
        Some(lampstand_api::RoleAskingView::Waiting)
    );
    send_ok(
        &laptop,
        TeamRequest::RoleClaim {
            role: RoleName::Stream,
        },
    )
    .await;
    assert!(role(&laptop, RoleName::Stream).holds);
    assert!(role(&laptop, RoleName::Stream).asking.is_none());
}

/// "Always allow" over the IPC stores nothing for Projection and Audio,
/// whatever the request says.
#[tokio::test(flavor = "multi_thread")]
async fn always_allow_is_refused_for_projection_and_audio() {
    let main = device("Sanctuary").await;
    let laptop = device("Mary's laptop").await;
    pair_up(&main, &laptop).await;
    for name in [RoleName::Projection, RoleName::Audio] {
        send_ok(
            &main,
            TeamRequest::RoleAlways {
                role: name,
                device: key(&laptop),
                always: true,
            },
        )
        .await;
        assert!(role(&main, name).always.is_empty(), "{name:?}");
        send_ok(&laptop, TeamRequest::RoleAsk { role: name }).await;
        until(Duration::from_secs(20), "the prompt", || {
            role(&main, name).request.is_some()
        })
        .await;
        send_ok(
            &main,
            TeamRequest::RoleAllow {
                role: name,
                device: key(&laptop),
                always: true,
            },
        )
        .await;
        until(Duration::from_secs(20), "the laptop to hold it", || {
            role(&laptop, name).holds
        })
        .await;
        assert!(role(&main, name).always.is_empty(), "{name:?}");
        assert!(!role(&main, name).offers_always);
    }
}
