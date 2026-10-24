//! Peer sync for the app: the local server's `WS /sync`, the sync node and
//! the `Team` requests behind the devices screen. The same server carries
//! the OBS browser sources, fed by the app's [`SceneHub`] (see [`Obs`]).
//!
//! Until teams (T18) a device syncs only with devices it has paired with:
//! the presenting machine shows a code (`StartPairing`), the service
//! device types it (`Join`). Changes then flow both ways as they happen.
//! A remote change reaches the app through the library's change feed like
//! a local one, and `App::library_changed` reloads the open service; the
//! live engine keeps what is on air (ARCHITECTURE.md, "Sync").
//!
//! Environment, for running two copies on one machine and for tests:
//! `LAMPSTAND_PORT` (default 7340; if taken, any free port),
//! `LAMPSTAND_DEVICE_NAME` (default: the host name) and
//! `LAMPSTAND_NO_MDNS`.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use lampstand_api::{
    AskingView, DeviceView, DriveAskingView, DriveView, DriverView, DrivingView, Event,
    LoadedService, NearbyView, ObsRequest, PairingView, PresentRequestView, PresenterRole,
    PresenterStanding, PresenterView, Request, Response, ServiceRequest, SyncState, SyncSummary,
    TeamRequest, TeamStatus,
};
use lampstand_core::id::BlobHash;
use lampstand_core::output::OutputId;
use lampstand_core::scene::Scene;
use lampstand_library::Library;
use lampstand_render::Renderer;
use lampstand_server::{Backend, SceneHub, Server};
use lampstand_sync::driver::{Asking, Notice as DriveNotice};
use lampstand_sync::net::{
    local_addresses, DriveHooks, DriveStatus, NodeConfig, NodeStatus, PresenterStatus, SyncNode,
};
use lampstand_sync::presenter::{Notice, Role, Standing};
use lampstand_sync::team::{DeviceKey, DeviceKeypair};

use crate::handler::{EventSink, SyncLink};

/// How long after a sync message the status reads "syncing". Once two
/// devices agree they send nothing, so this is the length of a burst.
const BUSY_MS: i64 = 1500;

/// How sync starts.
#[derive(Debug, Clone)]
pub struct SyncConfig {
    pub name: String,
    /// The local server's port; 0 for any free one.
    pub port: u16,
    pub mdns: bool,
}

impl SyncConfig {
    /// From the environment, as described in the module docs.
    pub fn from_env() -> Self {
        let name = std::env::var("LAMPSTAND_DEVICE_NAME")
            .ok()
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| {
                let host = gethostname::gethostname().to_string_lossy().into_owned();
                host.trim_end_matches(".local").to_string()
            });
        let port = std::env::var("LAMPSTAND_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(lampstand_server::DEFAULT_PORT);
        Self {
            name,
            port,
            mdns: std::env::var_os("LAMPSTAND_NO_MDNS").is_none(),
        }
    }
}

/// Told whether this device may drive the screens (`true`: it presents),
/// once at the start and whenever that changes. Runs on a sync task, so it
/// must not wait for the `App` lock.
pub type RoleSink = Arc<dyn Fn(bool) + Send + std::marker::Sync>;

/// What the presenting device does with driving, supplied by the shell:
/// both run on sync tasks and must not block.
pub struct DriveSinks {
    /// The active driver sent a request this device may carry out.
    pub command: Arc<dyn Fn(Request) + Send + std::marker::Sync>,
    /// A device started (`true`) or stopped driving this one.
    pub driver: Arc<dyn Fn(bool) + Send + std::marker::Sync>,
    /// Filled with the sync node once it exists, for the event sink made by
    /// [`mirroring`].
    pub cell: NodeCell,
}

impl Default for DriveSinks {
    /// Nothing to do: for a device that nobody drives (tests).
    fn default() -> Self {
        Self {
            command: Arc::new(|_| {}),
            driver: Arc::new(|_| {}),
            cell: NodeCell::default(),
        }
    }
}

/// Lets the event sink reach the sync node, which is made after it: the
/// presenting device mirrors its live events to the driver through it.
pub type NodeCell = Arc<OnceLock<Arc<SyncNode>>>;

/// `emit` plus, on a device that someone is driving, a copy of the events
/// the driver's screen mirrors: what is on air, the loaded service and OBS.
pub fn mirroring(emit: EventSink, node: NodeCell) -> EventSink {
    Arc::new(move |event: Event| {
        if matches!(event, Event::Live { .. } | Event::Loaded(_) | Event::Obs(_)) {
            if let Some(node) = node.get().filter(|n| n.driven()) {
                if let Ok(json) = serde_json::to_vec(&event) {
                    node.drive_live(json);
                }
            }
        }
        emit(event);
    })
}

/// Whether a driver may ask the presenting device to carry this request
/// out: the live commands, opening and closing the service on air, a loop
/// change on air and OBS scene and mute choices. Everything else (editing a
/// service, settings, screens) stays on the device it belongs to.
pub fn driveable(request: &Request) -> bool {
    matches!(
        request,
        Request::Live(_)
            | Request::Service(
                ServiceRequest::Open(_)
                    | ServiceRequest::Close(_)
                    | ServiceRequest::SetLiveLoop { .. }
            )
            | Request::Obs(
                ObsRequest::SetScene { .. } | ObsRequest::Resume | ObsRequest::SetMute { .. }
            )
    )
}

/// What the driver last heard of the loaded service, so opening one from
/// the driver can answer with it.
#[derive(Default)]
struct Mirrored {
    loaded: Mutex<Option<LoadedService>>,
    changed: tokio::sync::Notify,
}

/// What the local server's OBS routes read: the app's browser-source keys
/// and scenes, and a renderer for deck frames.
#[derive(Clone)]
pub struct Obs {
    pub hub: Arc<SceneHub>,
    /// An offscreen renderer of its own (not the render thread's), so
    /// deck slides go out as frames; without one they are drawn in the DOM.
    pub renderer: Option<Arc<Renderer>>,
}

/// The sync node and the server it listens on.
pub struct Sync {
    node: Arc<SyncNode>,
    server: Server,
    mirrored: Arc<Mirrored>,
}

struct AppBackend {
    node: Arc<SyncNode>,
    blobs: PathBuf,
    obs: Obs,
}

/// The app's view of the sync node (`SyncLink`): who is connected, and
/// fetching files from them.
struct NodeLink(Arc<SyncNode>);

impl SyncLink for NodeLink {
    fn device_name(&self) -> String {
        self.0.status().name
    }
    fn device_key(&self) -> String {
        self.0.device().to_hex()
    }
    fn connected(&self) -> Vec<String> {
        self.0
            .status()
            .paired
            .iter()
            .filter(|p| p.connected)
            .map(|p| p.device.to_hex())
            .collect()
    }
    fn fetch(&self, hashes: Vec<BlobHash>) {
        let node = self.0.clone();
        tauri::async_runtime::spawn(async move {
            for hash in hashes {
                if let Err(e) = node.fetch_blob(hash).await {
                    eprintln!("lampstand: could not fetch a file yet: {e}");
                }
            }
        });
    }
}

impl Backend for AppBackend {
    fn output_key_valid(&self, output: OutputId, key: &str) -> bool {
        self.obs.hub.key_valid(output, key)
    }
    fn watch_scene(&self, output: OutputId) -> Option<std::sync::mpsc::Receiver<Arc<Scene>>> {
        self.obs.hub.watch(output)
    }
    fn renderer(&self) -> Option<Arc<Renderer>> {
        self.obs.renderer.clone()
    }
    fn blob_path(&self, hash: &BlobHash) -> Option<PathBuf> {
        let path = self.blobs.join(hash.to_string());
        path.is_file().then_some(path)
    }
    fn sync(&self) -> Option<Arc<SyncNode>> {
        Some(self.node.clone())
    }
}

impl Sync {
    /// Starts the local server and the sync node over `library`, keeping
    /// `paired.json` in `data_dir`, and emits `Event::SyncStatus` whenever
    /// connections or nearby devices change. The server's OBS routes read
    /// `obs`. Call inside the Tokio runtime.
    #[allow(clippy::too_many_arguments)]
    pub async fn start(
        library: &Arc<Library>,
        key: DeviceKeypair,
        data_dir: &Path,
        config: SyncConfig,
        obs: Obs,
        emit: EventSink,
        on_role: RoleSink,
        drive: DriveSinks,
    ) -> Result<Self, String> {
        let node = SyncNode::new(
            library.shared_repo(),
            key,
            NodeConfig {
                name: config.name.clone(),
                dir: data_dir.join("sync"),
            },
        )
        .map_err(|e| e.to_string())?;
        let backend = Arc::new(AppBackend {
            node: node.clone(),
            blobs: data_dir.join("library/repo/blobs"),
            obs,
        });
        let any = SocketAddr::from(([0, 0, 0, 0], config.port));
        let server = match Server::start(any, backend.clone()).await {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse && config.port != 0 => {
                eprintln!(
                    "lampstand: port {} is taken; the local server uses another",
                    config.port
                );
                Server::start(SocketAddr::from(([0, 0, 0, 0], 0)), backend)
                    .await
                    .map_err(|e| e.to_string())?
            }
            Err(e) => return Err(format!("cannot start the local server: {e}")),
        };
        let port = server.local_addr().port();
        let mirrored = Arc::new(Mirrored::default());
        let (command, driver, cell) = (drive.command, drive.driver, drive.cell);
        let (live_emit, live_mirror) = (emit.clone(), mirrored.clone());
        node.set_drive_hooks(DriveHooks {
            command: Box::new(move |from, payload| {
                match serde_json::from_slice::<Request>(&payload) {
                    Ok(request) if driveable(&request) => command(request),
                    Ok(request) => eprintln!(
                        "lampstand: {} asked for something a driver may not do: {request:?}",
                        from.fingerprint()
                    ),
                    Err(e) => eprintln!("lampstand: unreadable request from a driver: {e}"),
                }
            }),
            live: Box::new(
                move |payload| match serde_json::from_slice::<Event>(&payload) {
                    Ok(event) => {
                        if let Event::Loaded(loaded) = &event {
                            *live_mirror.loaded.lock().unwrap_or_else(|e| e.into_inner()) =
                                Some(loaded.clone());
                            live_mirror.changed.notify_waiters();
                        }
                        live_emit(event);
                    }
                    Err(e) => eprintln!("lampstand: unreadable state from the presenter: {e}"),
                },
            ),
            driver: Box::new(move |on| driver(on)),
        });
        let _ = cell.set(node.clone());
        node.start(port, config.mdns);
        eprintln!(
            "lampstand: syncing as {:?} ({}) on port {port}",
            config.name,
            node.device().fingerprint()
        );
        let mut changes = node.watch();
        let watched = node.clone();
        let mut presenting = watched.role() == Role::Presenter;
        on_role(presenting);
        tokio::spawn(async move {
            let mut last = None;
            let mut busy = false;
            loop {
                if busy {
                    // Say "synced" once the burst is over, though nothing
                    // else has changed.
                    tokio::select! {
                        r = changes.changed() => if r.is_err() { break },
                        _ = tokio::time::sleep(Duration::from_millis(BUSY_MS as u64 + 100)) => {}
                    }
                } else if changes.changed().await.is_err() {
                    break;
                }
                // Changes come in bursts (a connection and its addresses).
                tokio::time::sleep(Duration::from_millis(50)).await;
                changes.borrow_and_update();
                let now_presenting = watched.role() == Role::Presenter;
                if now_presenting != presenting {
                    presenting = now_presenting;
                    eprintln!(
                        "lampstand: this device is {} now",
                        if presenting { "presenting" } else { "planning" }
                    );
                    on_role(presenting);
                }
                let status = watched.status();
                let sync = summarize(&status, now_ms());
                busy = sync.state == SyncState::Syncing;
                let peers = sync.connected;
                emit(Event::SyncStatus { sync });
                if last != Some(peers) {
                    eprintln!("lampstand: {peers} paired device(s) connected");
                    last = Some(peers);
                }
            }
        });
        Ok(Self {
            node,
            server,
            mirrored,
        })
    }

    /// What the handler needs of the node, for the settings shared with
    /// the church's other devices.
    pub fn link(&self) -> Arc<dyn SyncLink> {
        Arc::new(NodeLink(self.node.clone()))
    }

    pub fn node(&self) -> &Arc<SyncNode> {
        &self.node
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.server.local_addr()
    }

    /// `host:port` for URLs other machines (OBS) open: this machine's LAN
    /// address, else localhost.
    pub fn public_address(&self) -> String {
        let port = self.local_addr().port();
        local_addresses(port)
            .into_iter()
            .next()
            .unwrap_or_else(|| format!("127.0.0.1:{port}"))
    }

    /// On a device driving another: carries `request` to the presenting
    /// device and answers for it. `None` when this device is not driving or
    /// the request is not one a driver may make: the caller handles it
    /// itself. Live state comes back as events, so a live command answers
    /// `Ok` at once; opening a service answers with the slides it loads.
    pub async fn forward(&self, request: &Request) -> Option<Response> {
        if !driveable(request) {
            return None;
        }
        let driving = self.node.driving()?;
        let error = |message: String| Some(Response::Error { message });
        if !driving.online {
            return error(format!(
                "{} is not connected. Controls wait until it is back.",
                driving.name
            ));
        }
        let payload = match serde_json::to_vec(request) {
            Ok(p) => p,
            Err(e) => return error(e.to_string()),
        };
        let open = match request {
            Request::Service(ServiceRequest::Open(id)) => Some(*id),
            _ => None,
        };
        // Look before sending, so a load that arrives at once is not missed.
        let waiting = self.mirrored.changed.notified();
        tokio::pin!(waiting);
        waiting.as_mut().enable();
        if let Err(e) = self.node.drive_send(payload) {
            return error(e);
        }
        let Some(id) = open else {
            return Some(Response::Ok);
        };
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let loaded = self
                .mirrored
                .loaded
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            if let Some(loaded) = loaded.filter(|l| l.service == id) {
                return Some(Response::Loaded(loaded));
            }
            if tokio::time::timeout_at(deadline, waiting.as_mut())
                .await
                .is_err()
            {
                return error(format!("{} did not open the service", driving.name));
            }
            waiting.set(self.mirrored.changed.notified());
            waiting.as_mut().enable();
        }
    }

    pub async fn handle(&self, request: TeamRequest) -> Response {
        let result = match request {
            TeamRequest::Status => Ok(()),
            TeamRequest::StartPairing => {
                self.node.start_pairing();
                Ok(())
            }
            TeamRequest::StopPairing => {
                self.node.stop_pairing();
                Ok(())
            }
            TeamRequest::Join { invite, address } => self
                .node
                .pair(&invite, address.as_deref())
                .await
                .map(|_| ())
                .map_err(|e| format!("Pairing failed: {e}")),
            TeamRequest::Connect { address } => self
                .node
                .connect(&address)
                .await
                .map(|_| ())
                .map_err(|e| format!("Cannot connect: {e}")),
            TeamRequest::Unpair { device } => match DeviceKey::from_hex(&device) {
                Some(key) => self.node.unpair(key).map(|_| ()).map_err(|e| e.to_string()),
                None => Err("not a device key".into()),
            },
            TeamRequest::PresenterClaim => self.node.claim_presenter().map_err(|e| e.to_string()),
            TeamRequest::PresenterAsk => self.node.ask_to_present().map_err(|e| e.to_string()),
            TeamRequest::PresenterCancel => {
                self.node.cancel_ask();
                Ok(())
            }
            TeamRequest::PresenterAllow { device } => match DeviceKey::from_hex(&device) {
                Some(key) => self.node.allow_presenter(key),
                None => Err("not a device key".into()),
            },
            TeamRequest::PresenterDecline { device } => match DeviceKey::from_hex(&device) {
                Some(key) => {
                    self.node.decline_presenter(key);
                    Ok(())
                }
                None => Err("not a device key".into()),
            },
            TeamRequest::PresenterDismiss => {
                self.node.dismiss_presenter_notice();
                Ok(())
            }
            TeamRequest::SetTakeover { seconds } => {
                self.node
                    .set_takeover_after(Duration::from_secs(u64::from(seconds)));
                Ok(())
            }
            TeamRequest::DriveAsk => self.node.drive_ask(),
            TeamRequest::DriveCancel => {
                self.node.drive_cancel();
                Ok(())
            }
            TeamRequest::DriveDismiss => {
                self.node.drive_dismiss();
                Ok(())
            }
            TeamRequest::DriveAllow { device, always } => match DeviceKey::from_hex(&device) {
                Some(key) => self.node.drive_allow(key, always),
                None => Err("not a device key".into()),
            },
            TeamRequest::DriveDecline { device } => match DeviceKey::from_hex(&device) {
                Some(key) => {
                    self.node.drive_decline(key);
                    Ok(())
                }
                None => Err("not a device key".into()),
            },
            TeamRequest::DriveTakeBack => {
                self.node.drive_take_back();
                Ok(())
            }
            TeamRequest::DriveAlways { device, always } => match DeviceKey::from_hex(&device) {
                Some(key) => self.node.drive_set_always(key, always),
                None => Err("not a device key".into()),
            },
            TeamRequest::CreateInvite { .. } => Err("team invites arrive with T18".into()),
            TeamRequest::SetRelay { .. } => Err("the relay arrives with T19".into()),
        };
        match result {
            Ok(()) => Response::Team(self.status()),
            Err(message) => Response::Error { message },
        }
    }

    pub fn status(&self) -> TeamStatus {
        view(&self.node.status(), self.local_addr().port(), now_ms())
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// The one honest state for the status bar, from what the node knows.
/// Worst first: a paired device that refuses this one (`Error`), a paired
/// device that is not connected (`Offline`), changes moving within
/// [`BUSY_MS`] (`Syncing`), else `Synced`; with nothing paired, `DeviceOnly`.
pub fn summarize(status: &NodeStatus, now_ms: i64) -> SyncSummary {
    let paired = status.paired.len();
    let connected = status.connected();
    let refused = status.paired.iter().find(|p| p.refused);
    let recent = [status.last_in_ms, status.last_out_ms]
        .into_iter()
        .flatten()
        .max()
        .is_some_and(|t| now_ms - t < BUSY_MS);
    let (state, reason) = if paired == 0 {
        (SyncState::DeviceOnly, None)
    } else if let Some(p) = refused {
        (
            SyncState::Error,
            Some(format!(
                "{} no longer accepts this device. Unpair it on both devices and pair again.",
                if p.name.is_empty() {
                    "A paired device"
                } else {
                    &p.name
                }
            )),
        )
    } else if connected < paired {
        (SyncState::Offline, None)
    } else if recent {
        (SyncState::Syncing, None)
    } else {
        (SyncState::Synced, None)
    };
    SyncSummary {
        state,
        paired,
        connected,
        reason,
        last_in_ms: status.last_in_ms,
        last_out_ms: status.last_out_ms,
    }
}

fn view(status: &NodeStatus, port: u16, now_ms: i64) -> TeamStatus {
    TeamStatus {
        name: status.name.clone(),
        fingerprint: status.device.fingerprint(),
        addresses: local_addresses(port),
        pairing: status.pairing.as_ref().map(|p| PairingView {
            code: p.code.clone(),
            expires_ms: p.expires_ms,
        }),
        paired: status
            .paired
            .iter()
            .map(|p| DeviceView {
                device: p.device.to_hex(),
                fingerprint: p.device.fingerprint(),
                name: p.name.clone(),
                address: p.address.clone(),
                connected: p.connected,
                presenting: p.presenting,
                problem: p.problem.clone(),
            })
            .collect(),
        nearby: status
            .nearby
            .iter()
            .map(|n| NearbyView {
                device: n.device.to_hex(),
                fingerprint: n.device.fingerprint(),
                name: n.name.clone(),
                address: n.address.clone(),
                pairing: n.pairing,
            })
            .collect(),
        presenter: presenter_view(&status.presenter),
        sync: summarize(status, now_ms),
    }
}

fn presenter_view(p: &PresenterStatus) -> PresenterView {
    let (standing, offline_secs, wait_secs) = match p.standing {
        Standing::Presenting => (PresenterStanding::Presenting, None, None),
        Standing::Open => (PresenterStanding::Open, None, None),
        Standing::Ask => (PresenterStanding::Ask, None, None),
        Standing::TakeOver { offline } => {
            (PresenterStanding::TakeOver, Some(offline.as_secs()), None)
        }
        Standing::Wait { offline, remaining } => (
            PresenterStanding::Wait,
            Some(offline.as_secs()),
            Some(remaining.as_secs() + 1),
        ),
    };
    PresenterView {
        role: match p.role {
            Role::Presenter => PresenterRole::Presenting,
            Role::Planning => PresenterRole::Planning,
        },
        standing,
        holder: p.lease.holder.map(|h| h.to_hex()),
        holder_name: p.lease.name.clone(),
        epoch: p.lease.epoch,
        offline_secs,
        wait_secs,
        takeover_secs: p.takeover_after.as_secs(),
        requests: p
            .requests
            .iter()
            .map(|r| PresentRequestView {
                device: r.device.to_hex(),
                name: r.name.clone(),
            })
            .collect(),
        asking: p.asking.map(|a| AskingView {
            declined: a.declined,
        }),
        replaced_by: p.notice.as_ref().map(|n| match n {
            Notice::Replaced { by } => by.clone(),
        }),
        drive: Box::new(drive_view(&p.drive)),
    }
}

fn drive_view(d: &DriveStatus) -> DriveView {
    let named = |n: &lampstand_sync::driver::Named| DriverView {
        device: n.device.to_hex(),
        name: n.name.clone(),
    };
    DriveView {
        driving: d.driving.as_ref().map(|g| DrivingView {
            presenter: g.presenter.to_hex(),
            name: g.name.clone(),
            online: g.online,
        }),
        active: d.active.as_ref().map(named),
        requests: d
            .requests
            .iter()
            .map(|r| PresentRequestView {
                device: r.device.to_hex(),
                name: r.name.clone(),
            })
            .collect(),
        asking: d.asking.as_ref().map(|a| match a {
            Asking::Waiting => DriveAskingView::Waiting,
            Asking::Declined => DriveAskingView::Declined,
            Asking::Busy { by } => DriveAskingView::Busy { by: by.clone() },
        }),
        can_drive: d.can_drive,
        always: d.always.iter().map(named).collect(),
        taken_back_by: d.notice.as_ref().map(|n| match n {
            DriveNotice::TakenBack { by } => by.clone(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lampstand_sync::net::PeerStatus;

    fn peer(name: &str, connected: bool, refused: bool) -> PeerStatus {
        PeerStatus {
            device: DeviceKeypair::generate().public(),
            name: name.into(),
            address: None,
            connected,
            presenting: false,
            problem: refused.then(|| "refused".into()),
            refused,
        }
    }

    fn status(paired: Vec<PeerStatus>, last_in_ms: Option<i64>) -> NodeStatus {
        NodeStatus {
            device: DeviceKeypair::generate().public(),
            name: "Sanctuary".into(),
            port: Some(7340),
            pairing: None,
            paired,
            nearby: Vec::new(),
            presenter: PresenterStatus {
                role: Role::Presenter,
                standing: Standing::Presenting,
                lease: Default::default(),
                takeover_after: std::time::Duration::from_secs(120),
                requests: Vec::new(),
                asking: None,
                notice: None,
                drive: Default::default(),
            },
            last_in_ms,
            last_out_ms: None,
        }
    }

    #[test]
    fn nothing_paired_is_this_device_only() {
        let s = summarize(&status(vec![], None), 10_000);
        assert_eq!(s.state, SyncState::DeviceOnly);
        assert_eq!((s.paired, s.connected), (0, 0));
    }

    #[test]
    fn connected_peers_are_counted_and_synced_when_quiet() {
        let two = vec![peer("Office", true, false), peer("Laptop", true, false)];
        let s = summarize(&status(two, Some(1_000)), 10_000);
        assert_eq!(s.state, SyncState::Synced);
        assert_eq!((s.paired, s.connected), (2, 2));
        assert_eq!(s.last_in_ms, Some(1_000));
    }

    #[test]
    fn recent_traffic_reads_as_syncing_then_settles() {
        let one = || vec![peer("Office", true, false)];
        let moving = summarize(&status(one(), Some(9_500)), 10_000);
        assert_eq!(moving.state, SyncState::Syncing);
        let settled = summarize(&status(one(), Some(9_500)), 9_500 + BUSY_MS);
        assert_eq!(settled.state, SyncState::Synced);
    }

    #[test]
    fn a_paired_device_not_found_is_offline() {
        let mixed = vec![peer("Office", true, false), peer("Laptop", false, false)];
        let s = summarize(&status(mixed, Some(9_900)), 10_000);
        assert_eq!(s.state, SyncState::Offline, "offline outranks syncing");
        assert_eq!((s.paired, s.connected), (2, 1));
    }

    #[test]
    fn a_device_that_refuses_this_one_is_an_error_with_a_reason() {
        let s = summarize(&status(vec![peer("Office", false, true)], None), 10_000);
        assert_eq!(s.state, SyncState::Error);
        assert!(s.reason.unwrap().contains("Office no longer accepts"));
    }
}
