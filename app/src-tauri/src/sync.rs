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
    DeviceView, Event, NearbyView, PairingView, Response, SyncState, SyncSummary, TeamRequest,
    TeamStatus,
};
use lampstand_core::id::BlobHash;
use lampstand_core::output::OutputId;
use lampstand_core::scene::Scene;
use lampstand_library::Library;
use lampstand_render::Renderer;
use lampstand_server::{Backend, SceneHub, Server};
use lampstand_sync::net::{local_addresses, NodeConfig, NodeStatus, SyncNode};
use lampstand_sync::team::{DeviceKey, DeviceKeypair};

use crate::handler::EventSink;

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
                if p.name.is_empty() { "A paired device" } else { &p.name }
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
        sync: summarize(status, now_ms),
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
