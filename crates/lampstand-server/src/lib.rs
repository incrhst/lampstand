//! The local server: everything the desktop app serves on the church
//! network, on one port (default 7340).
//!
//! PRD: OB-2, CD-2, PS-2 (picker page); Phase 2 adds the phone remote and
//! congregation view (LV-5).
//!
//! Routes:
//!
//! | Route | What |
//! | --- | --- |
//! | `GET /obs/<output>?k=<key>` | OBS browser source page: transparent, draws the stream output's scene in the DOM |
//! | `WS /obs/<output>/ws?k=<key>` | that output's scenes as JSON, pushed on every change |
//! | `GET /blobs/<hash>?k=<key>` | images and fonts for the browser source |
//! | `GET /frames/<output>/<n>.webp?k=<key>` | deck slides rasterised by the renderer, for the browser source |
//! | `WS /sync` | peer sync (device-key handshake, see `lampstand-sync`) |
//! | `GET /google/picker` | the Google Drive picker page, localhost only |
//!
//! `k` is a per-output random key shown in the app with the URL to paste
//! into OBS, so other machines on the network cannot read the stream.
//! Text in the browser source uses the same bundled fonts, served as
//! blobs, so lower thirds match the projector.

//! T42 builds only what peer sync needs: `Server::start` and `WS /sync`.
//! The OBS routes (T43) and the picker page (T40) answer 404 until then.

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use lampstand_core::id::BlobHash;
use lampstand_core::output::OutputId;
use lampstand_core::scene::Scene;
use lampstand_sync::net::{Link, SyncNode};
use tokio::sync::oneshot;

pub const DEFAULT_PORT: u16 = lampstand_sync::net::DEFAULT_PORT;

/// What the server needs from the app.
pub trait Backend: Send + Sync + 'static {
    /// Checks a browser-source key.
    fn output_key_valid(&self, output: OutputId, key: &str) -> bool;
    /// The current scene for a browser-source output, and a way to wait for
    /// the next one.
    fn watch_scene(&self, output: OutputId) -> Option<std::sync::mpsc::Receiver<Arc<Scene>>>;
    fn blob_path(&self, hash: &BlobHash) -> Option<std::path::PathBuf>;
    /// The sync node that takes `WS /sync` connections; `None` answers 404.
    fn sync(&self) -> Option<Arc<SyncNode>> {
        None
    }
}

/// The running server. Dropping it stops listening; connections already
/// open finish on their own.
pub struct Server {
    addr: SocketAddr,
    _stop: oneshot::Sender<()>,
}

impl Server {
    /// Binds `addr` and serves in the background on the current Tokio
    /// runtime. Port 0 picks a free port; see [`Server::local_addr`].
    pub async fn start(addr: SocketAddr, backend: Arc<dyn Backend>) -> io::Result<Self> {
        let listener = tokio::net::TcpListener::bind(addr).await?;
        let addr = listener.local_addr()?;
        let app = Router::new()
            .route("/sync", get(sync))
            .fallback(|| async { StatusCode::NOT_FOUND })
            .with_state(backend);
        let (stop, stopped) = oneshot::channel::<()>();
        tokio::spawn(async move {
            let serve = axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(async move {
                let _ = stopped.await;
            });
            if let Err(e) = serve.await {
                eprintln!("lampstand: the local server stopped: {e}");
            }
        });
        Ok(Self { addr, _stop: stop })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }
}

/// `WS /sync`: upgrades and hands the socket to the sync node, which runs
/// the device-key handshake and refuses devices that are not paired.
async fn sync(
    State(backend): State<Arc<dyn Backend>>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    ws: WebSocketUpgrade,
) -> Response {
    let Some(node) = backend.sync() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    ws.on_upgrade(move |socket| async move {
        node.accept(link(socket, remote)).await;
    })
}

/// An axum WebSocket as a sync link: binary messages only.
fn link(socket: WebSocket, remote: SocketAddr) -> Link {
    let (tx, rx) = socket.split();
    let tx = tx
        .sink_map_err(io::Error::other)
        .with(|bytes: Vec<u8>| async move { Ok::<_, io::Error>(Message::Binary(bytes.into())) });
    let rx = rx
        .take_while(|m| std::future::ready(!matches!(m, Ok(Message::Close(_)))))
        .filter_map(|m| async move {
            match m {
                Ok(Message::Binary(bytes)) => Some(Ok(bytes.to_vec())),
                Ok(_) => None,
                Err(e) => Some(Err(io::Error::other(e))),
            }
        });
    Link::new(tx, rx, Some(remote))
}
