//! Peer sync for the app: the local server's `WS /sync`, the sync node and
//! the `Team` requests behind the devices screen.
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
    DeviceView, Event, NearbyView, PairingView, Response, TeamRequest, TeamStatus,
};
use lampstand_core::id::BlobHash;
use lampstand_core::output::OutputId;
use lampstand_core::scene::Scene;
use lampstand_library::Library;
use lampstand_server::{Backend, Server};
use lampstand_sync::net::{local_addresses, NodeConfig, NodeStatus, SyncNode};
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

/// The sync node and the server it listens on.
pub struct Sync {
    node: Arc<SyncNode>,
    server: Server,
}

struct AppBackend {
    node: Arc<SyncNode>,
    blobs: PathBuf,
}

impl Backend for AppBackend {
    // Browser-source outputs arrive with T43.
    fn output_key_valid(&self, _: OutputId, _: &str) -> bool {
        false
    }
    fn watch_scene(&self, _: OutputId) -> Option<std::sync::mpsc::Receiver<Arc<Scene>>> {
        None
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
    /// connections or nearby devices change. Call inside the Tokio runtime.
    pub async fn start(
        library: &Arc<Library>,
        key: DeviceKeypair,
        data_dir: &Path,
        config: SyncConfig,
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
            while changes.changed().await.is_ok() {
                // Changes come in bursts (a connection and its addresses).
                tokio::time::sleep(Duration::from_millis(50)).await;
                changes.borrow_and_update();
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
    }
}
