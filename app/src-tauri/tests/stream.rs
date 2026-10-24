//! The Stream holder owns the OBS connection, end to end across apps
//! (decision 0010, point 6): the Projection holder sends each live item's
//! scene as a cue; the Stream holder applies it to its own OBS; Projection
//! never switches OBS itself while Stream is held; with nobody on Stream it
//! keeps the duty as in 0.7.0; and a church with one device sees no change.
//!
//! Each device runs the shell's wiring (`stream_cues`) with a recording sink
//! where the shell has the OBS connection, so what is asserted is exactly
//! which device's OBS would have been told to switch. The OBS follower
//! itself, and the live connection, are tested in `obs_control.rs`.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use lampstand_api::{
    Event, LibraryRequest, ObsRequest, ObsView, OutputRequest, Request, Response, ServiceRequest,
    TeamRequest,
};
use lampstand_app::config::device_key;
use lampstand_app::handler::{App, Placement, WindowHost};
use lampstand_app::obs_control::{ObsControl, Os, SceneCue, System};
use lampstand_app::stream_cues;
use lampstand_app::sync::{mirroring, DriveSinks, NodeCell, Obs, Sync, SyncConfig};
use lampstand_core::output::{BackgroundMode, OutputConfig, OutputId, OutputRole, OutputTarget};
use lampstand_core::safe_area::SafeArea;
use lampstand_core::service::{ItemId, ItemKind};
use lampstand_layout::Fonts;
use lampstand_live::Command;
use lampstand_render::output::DisplayInfo;
use lampstand_render::{OutputCommand, OutputSink};
use lampstand_sync::roles::Role;
use tokio::sync::mpsc;

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

/// One copy of the app with the shell's OBS wiring.
struct Device {
    app: App,
    sync: Sync,
    obs: Arc<ObsControl>,
    events: Arc<Mutex<Vec<Event>>>,
    /// What this device's OBS would be told to do (the shell hands these to
    /// `ObsControl`).
    cues: Arc<Mutex<Vec<Option<String>>>>,
    /// Requests a driver sent, for `pump`.
    driven: Arc<Mutex<Vec<Request>>>,
    cell: NodeCell,
    emit: lampstand_app::handler::EventSink,
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
    let mut app = App::open(
        dir.path(),
        Fonts::bundled_only(),
        Arc::new(Sink),
        Box::new(Host),
        emit.clone(),
    )
    .unwrap();
    app.handle(Request::Outputs(OutputRequest::Put(screen(name))));

    // As the shell does: OBS control with shaped events, a recorder where
    // its cue channel would feed the OBS follower, and the router in front.
    let obs = ObsControl::start(
        dir.path(),
        Arc::new(AtomicBool::new(false)),
        Arc::new(System),
        Os::current(),
        stream_cues::shaping(emit.clone(), cell.clone()),
    );
    let (local, mut local_rx) = mpsc::unbounded_channel::<SceneCue>();
    let cues = Arc::new(Mutex::new(Vec::new()));
    let recording = cues.clone();
    tokio::spawn(async move {
        while let Some(cue) = local_rx.recv().await {
            recording.lock().unwrap().push(cue.scene);
        }
    });
    let view_of = obs.clone();
    let routed = stream_cues::start_router(
        cell.clone(),
        local.clone(),
        Arc::new(move || view_of.view()),
        emit.clone(),
    );
    app.set_obs_cues(routed);

    let driven = Arc::new(Mutex::new(Vec::new()));
    let queued = driven.clone();
    let drive_cell = cell.clone();
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
        emit.clone(),
        Arc::new(|_| {}),
        DriveSinks {
            command: Arc::new(move |request| match request {
                // As the shell: OBS requests count only where OBS is run.
                Request::Obs(_) if !stream_cues::obs_here(&drive_cell) => {}
                request => queued.lock().unwrap().push(request),
            }),
            driver: Arc::new(|_| {}),
            cell: cell.clone(),
        },
    )
    .await
    .unwrap();
    stream_cues::accept(sync.node(), local);
    app.set_sync_link(sync.link());
    Device {
        app,
        sync,
        obs,
        events,
        cues,
        driven,
        cell,
        emit,
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

    fn address(&self) -> String {
        format!("127.0.0.1:{}", self.sync.local_addr().port())
    }

    fn node(&self) -> &Arc<lampstand_sync::net::SyncNode> {
        self.sync.node()
    }

    /// The scenes this device's OBS was told to switch to (none-cues, which
    /// only end a hand pick, left out).
    fn switched(&self) -> Vec<String> {
        self.cues
            .lock()
            .unwrap()
            .iter()
            .flatten()
            .cloned()
            .collect()
    }

    /// Every cue, a new item without a scene included.
    fn all_cues(&self) -> Vec<Option<String>> {
        self.cues.lock().unwrap().clone()
    }

    /// The OBS views this device told its screen about, newest last.
    fn obs_views(&self) -> Vec<ObsView> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                Event::Obs(v) => Some((**v).clone()),
                _ => None,
            })
            .collect()
    }

    /// What the app thread does for a driver's requests.
    fn pump(&mut self) {
        for request in std::mem::take(&mut *self.driven.lock().unwrap()) {
            if let Response::Error { message } = self.app.handle(request) {
                panic!("a driver's request failed: {message}");
            }
        }
    }
}

async fn until(limit: Duration, what: &str, mut check: impl FnMut() -> bool) {
    let start = Instant::now();
    while !check() {
        assert!(start.elapsed() < limit, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

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
        other.node().roles_status().audio.record.holder == Some(main.node().device())
            && main.node().status().connected() >= 1
            && other.node().status().connected() >= 1
    })
    .await;
    // Both devices dial at pairing, and the connection that loses the
    // tie-break is closed a moment later: let that settle, so a role claimed
    // next is not sent down the one that goes.
    tokio::time::sleep(Duration::from_millis(1500)).await;
}

/// A service with two songs, the first naming the scene "Worship" and the
/// second "Sermon", open on `d`.
fn service_with_scenes(d: &mut Device) -> (ItemId, ItemId) {
    let song = |d: &mut Device| match d.request(Request::Library(LibraryRequest::SearchSongs {
        query: "Amazing Grace".into(),
        limit: 1,
    })) {
        Response::Songs(hits) => hits[0].song,
        other => panic!("{other:?}"),
    };
    let grace = song(d);
    let service = match d.request(Request::Service(ServiceRequest::Create {
        title: "Sunday".into(),
        starts_at: None,
        time_zone: None,
    })) {
        Response::Service(s) => s,
        other => panic!("{other:?}"),
    };
    let mut ids = Vec::new();
    for scene in ["Worship", "Sermon"] {
        let service = match d.request(Request::Service(ServiceRequest::InsertItem {
            service: service.id,
            index: usize::MAX,
            kind: ItemKind::Song {
                song: grace,
                arrangement: None,
            },
        })) {
            Response::Service(s) => s,
            other => panic!("{other:?}"),
        };
        let item = service.ordered().last().unwrap().0;
        d.request(Request::Service(ServiceRequest::SetObsScene {
            service: service.id,
            item,
            scene: Some(scene.into()),
        }));
        ids.push(item);
    }
    d.request(Request::Service(ServiceRequest::Open(service.id)));
    (ids[0], ids[1])
}

fn next(d: &mut Device) {
    d.request(Request::Live(Command::NextItem));
}

/// Waits for the cue router (a task) to have handled what the app sent.
async fn settle() {
    tokio::time::sleep(Duration::from_millis(200)).await;
}

/// A church with one device and nothing paired: its own OBS follows its
/// items exactly as in 0.7.0, its screen is told what 0.7.0 told it, and a
/// scene picked by hand is carried out.
#[tokio::test(flavor = "multi_thread")]
async fn a_church_with_one_device_and_no_pairing_drives_its_own_obs_as_before() {
    let mut lone = device("Sanctuary").await;
    service_with_scenes(&mut lone);
    next(&mut lone);
    next(&mut lone);
    settle().await;
    assert_eq!(
        lone.all_cues(),
        [Some("Worship".to_string()), Some("Sermon".to_string())]
    );
    assert!(stream_cues::obs_here(&lone.cell));
    assert!(lone.node().obs_here() && lone.node().stream_elsewhere().is_none());
    // The view is the one OBS control made: nothing hidden. (OBS settings
    // saved or a stream output make it relevant; shaping never removes that
    // here.)
    let own = lone.obs.view();
    assert_eq!(stream_cues::shape(own.clone(), true), own);
    let shaped = stream_cues::shape_response(Response::Obs(own.clone()), true);
    assert_eq!(shaped, Response::Obs(own));
    // Nothing waits anywhere, and nothing was written for roles.
    assert!(!lone.node().scene_cue_waiting());
    assert!(!lone._dir.path().join("sync/roles.json").exists());
}

/// Paired, but nobody has taken Stream: Projection keeps the OBS duty, the
/// other device does nothing with OBS.
#[tokio::test(flavor = "multi_thread")]
async fn with_nobody_holding_stream_projection_keeps_the_obs_duty() {
    let mut main = device("Sanctuary").await;
    let office = device("Office PC").await;
    pair_up(&main, &office).await;
    service_with_scenes(&mut main);
    next(&mut main);
    settle().await;
    assert_eq!(main.switched(), ["Worship"]);
    assert!(office.switched().is_empty());
    assert!(!main.node().scene_cue_waiting());
}

/// Two apps: the stream machine takes Stream; the projector's OBS is left
/// alone and the stream machine's is told, item by item.
#[tokio::test(flavor = "multi_thread")]
async fn the_stream_holder_applies_each_cue_and_projection_never_switches_obs() {
    let mut main = device("Sanctuary").await;
    let stream = device("Stream PC").await;
    pair_up(&main, &stream).await;
    stream.node().claim_role(Role::Stream).unwrap();
    until(Duration::from_secs(20), "Projection to know", || {
        main.node().stream_elsewhere().is_some()
    })
    .await;

    service_with_scenes(&mut main);
    next(&mut main);
    until(Duration::from_secs(5), "the first cue", || {
        stream.switched() == ["Worship"]
    })
    .await;
    next(&mut main);
    until(Duration::from_secs(5), "the second cue", || {
        stream.switched() == ["Worship", "Sermon"]
    })
    .await;
    settle().await;
    assert!(
        main.switched().is_empty(),
        "Projection never switches OBS while Stream is held: {:?}",
        main.all_cues()
    );
    until(Duration::from_secs(5), "the answer", || {
        !main.node().scene_cue_waiting()
    })
    .await;

    // What each screen is told: only the Stream holder's OBS is worth
    // mentioning.
    let hidden = |d: &Device| d.obs_views().last().is_some_and(|v| !v.relevant);
    assert!(hidden(&main), "no OBS strip on the projector machine");
    let on_stream = stream.obs.view();
    assert_eq!(stream_cues::shape(on_stream.clone(), true), on_stream);

    // A scene or mute picked by hand is refused where OBS is not run.
    assert!(!main.node().obs_here());
    assert!(stream.node().obs_here());
}

/// Moving Stream moves the duty: handed to the projector machine, it is
/// Projection's own OBS again, and passing Stream replays no cue.
#[tokio::test(flavor = "multi_thread")]
async fn when_stream_moves_to_the_projector_machine_the_duty_moves_with_it() {
    let mut main = device("Sanctuary").await;
    let stream = device("Stream PC").await;
    pair_up(&main, &stream).await;
    stream.node().claim_role(Role::Stream).unwrap();
    until(Duration::from_secs(20), "Projection to know", || {
        !main.node().obs_here()
    })
    .await;
    service_with_scenes(&mut main);
    next(&mut main);
    until(Duration::from_secs(5), "the cue", || {
        stream.switched() == ["Worship"]
    })
    .await;
    // The projector machine asks for Stream; the stream machine allows.
    let began = Instant::now();
    while stream.node().roles_status().stream.request.is_none() {
        assert!(began.elapsed() < Duration::from_secs(15), "no prompt");
        main.node().ask_role(Role::Stream).unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    stream
        .node()
        .allow_role(Role::Stream, main.node().device(), false)
        .unwrap();
    until(Duration::from_secs(5), "the duty to move back", || {
        main.node().obs_here()
    })
    .await;
    assert!(!stream.node().obs_here());
    settle().await;
    // Passing Stream leaves OBS where it is: nothing was replayed.
    assert!(main.switched().is_empty());
    // The next item is Projection's own again, and the stream machine's OBS
    // hears nothing more.
    next(&mut main);
    until(Duration::from_secs(5), "its own cue", || {
        main.switched() == ["Sermon"]
    })
    .await;
    settle().await;
    assert_eq!(stream.switched(), ["Worship"]);
    // The screens were told when the duty moved.
    until(
        Duration::from_secs(5),
        "the stream machine's screen to hear",
        || stream.obs_views().last().is_some_and(|v| !v.relevant),
    )
    .await;
}

/// Three apps: the projector, a laptop at the back that runs the service,
/// and the stream machine. The laptop's presses reach the projector, whose
/// cue reaches the stream machine; the laptop's scene picks do not reach the
/// projector's OBS, and the projector's OBS state is not mirrored to it.
#[tokio::test(flavor = "multi_thread")]
async fn three_apps_cue_reaches_the_stream_machine_when_the_operator_presses_next() {
    let mut main = device("Sanctuary").await;
    let laptop = device("Mary's laptop").await;
    let stream = device("Stream PC").await;
    pair_up(&main, &laptop).await;
    pair_up(&main, &stream).await;
    // The stream machine and the laptop know each other too.
    let code = match stream.sync.handle(TeamRequest::StartPairing).await {
        Response::Team(t) => t.pairing.unwrap().code,
        other => panic!("{other:?}"),
    };
    laptop
        .sync
        .handle(TeamRequest::Join {
            invite: code,
            address: Some(stream.address()),
        })
        .await;
    until(Duration::from_secs(5), "everyone to connect", || {
        [&main, &laptop, &stream]
            .iter()
            .all(|d| d.node().status().connected() == 2)
    })
    .await;
    // The laptop takes the Operator seat (the Projection holder allows).
    assert!(matches!(
        laptop.sync.handle(TeamRequest::DriveAsk).await,
        Response::Team(_) | Response::Error { .. }
    ));
    until(Duration::from_secs(5), "the laptop to be asked", || {
        main.node()
            .status()
            .presenter
            .drive
            .requests
            .iter()
            .any(|r| r.device == laptop.node().device())
            || laptop.node().driving().is_some()
    })
    .await;
    if laptop.node().driving().is_none() {
        main.sync
            .handle(TeamRequest::DriveAllow {
                device: laptop.node().device().to_hex(),
                always: false,
            })
            .await;
    }
    until(Duration::from_secs(5), "the laptop to drive", || {
        laptop.node().driving().is_some_and(|d| d.online)
    })
    .await;
    // While nobody holds Stream the projector's OBS state is the operator's:
    // mirrored to its screen, as in 0.7.0.
    (main.emit)(Event::Obs(Box::new(main.obs.view())));
    until(
        Duration::from_secs(5),
        "the OBS state to be mirrored",
        || !laptop.obs_views().is_empty(),
    )
    .await;
    // The stream machine takes Stream.
    stream.node().claim_role(Role::Stream).unwrap();
    until(Duration::from_secs(5), "everyone to know", || {
        !main.node().obs_here()
            && main.node().stream_elsewhere().is_some()
            && laptop.node().stream_elsewhere().is_some()
    })
    .await;

    service_with_scenes(&mut main);
    // The laptop presses Next: forwarded to the projector, which goes live.
    assert!(laptop
        .sync
        .forward(&Request::Live(Command::NextItem))
        .await
        .is_some());
    until(Duration::from_secs(5), "the stream machine's cue", || {
        main.pump();
        stream.switched() == ["Worship"]
    })
    .await;
    assert!(main.switched().is_empty() && laptop.switched().is_empty());

    // A scene picked by hand on the laptop is not carried to the projector:
    // OBS belongs to the stream machine.
    let pick = Request::Obs(ObsRequest::SetScene {
        scene: "Wide".into(),
    });
    match laptop.sync.forward(&pick).await {
        Some(Response::Error { message }) => {
            assert_eq!(message, stream_cues::NOT_THE_STREAM)
        }
        other => panic!("{other:?}"),
    }
    settle().await;
    main.pump();
    assert!(main.driven.lock().unwrap().is_empty());
    // And were one to arrive, the projector would not act on it.
    assert!(!stream_cues::obs_here(&main.cell));

    // The projector's OBS state is not the operator's business now: it is not
    // mirrored, and the operator's screen shows no OBS strip.
    let before = laptop.obs_views().len();
    (main.emit)(Event::Obs(Box::new(main.obs.view())));
    settle().await;
    assert_eq!(laptop.obs_views().len(), before);
}
