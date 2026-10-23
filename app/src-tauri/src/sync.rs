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
use std::sync::Arc;
use std::time::Duration;

use lampstand_api::{
    AskingView, DeviceView, Event, NearbyView, PairingView, PresentRequestView, PresenterRole,
    PresenterStanding, PresenterView, Response, TeamRequest, TeamStatus,
};
use lampstand_core::id::BlobHash;
use lampstand_core::output::OutputId;
use lampstand_core::scene::Scene;
use lampstand_library::Library;
use lampstand_render::Renderer;
use lampstand_server::{Backend, SceneHub, Server};
use lampstand_sync::net::{local_addresses, NodeConfig, NodeStatus, PresenterStatus, SyncNode};
use lampstand_sync::presenter::{Notice, Role, Standing};
use lampstand_sync::team::{DeviceKey, DeviceKeypair};

use crate::handler::EventSink;

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
}

struct AppBackend {
    node: Arc<SyncNode>,
    blobs: PathBuf,
    obs: Obs,
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
    pub async fn start(
        library: &Arc<Library>,
        key: DeviceKeypair,
        data_dir: &Path,
        config: SyncConfig,
        obs: Obs,
        emit: EventSink,
        on_role: RoleSink,
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
            while changes.changed().await.is_ok() {
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
                let peers = watched.status().connected();
                emit(Event::SyncStatus {
                    peers,
                    relay_connected: false,
                });
                if last != Some(peers) {
                    eprintln!("lampstand: {peers} paired device(s) connected");
                    last = Some(peers);
                }
            }
        });
        Ok(Self { node, server })
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
            TeamRequest::CreateInvite { .. } => Err("team invites arrive with T18".into()),
            TeamRequest::SetRelay { .. } => Err("the relay arrives with T19".into()),
        };
        match result {
            Ok(()) => Response::Team(self.status()),
            Err(message) => Response::Error { message },
        }
    }

    pub fn status(&self) -> TeamStatus {
        view(&self.node.status(), self.local_addr().port())
    }
}

fn view(status: &NodeStatus, port: u16) -> TeamStatus {
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
    }
}
