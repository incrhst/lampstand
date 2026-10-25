//! The local server: everything the desktop app serves on the church
//! network, on one port (default 7340).
//!
//! PRD: OB-2, CD-2, PS-2 (picker page), LV-5 (Lampstand Remote; the
//! congregation view comes later).
//!
//! Routes:
//!
//! | Route | What |
//! | --- | --- |
//! | `GET /obs/<output>?k=<key>` | OBS browser source page: transparent, draws the stream output's scene in the DOM |
//! | `WS /obs/<output>/ws?k=<key>` | that output's scenes as JSON, pushed on every change |
//! | `GET /obs/assets/*` | the page's script and the bundled fonts it draws text with (public) |
//! | `GET /blobs/<hash>?o=<output>&k=<key>` | images and videos a scene names |
//! | `GET /frames/<output>/<n>.webp?k=<key>` | deck slides rasterised by the renderer, for the browser source |
//! | `WS /sync` | peer sync (device-key handshake, see `lampstand-sync`) |
//! | `GET /remote`, `POST /remote/pair`, `WS /remote/ws` | Lampstand Remote, paired by PIN (see [`remote`]) |
//! | `GET /google/picker` | the Google Drive picker page, localhost only |
//!
//! `k` is a per-output random key shown in the app with the URL to paste
//! into OBS, so other machines on the network cannot read the stream.
//! Text in the browser source uses the same bundled fonts, served from the
//! binary, so lower thirds match the projector. See [`obs`] for the page and
//! its feed, and [`SceneHub`] for what the app calls to feed it.
//!
//! T42 built `Server::start` and `WS /sync`, T43 the OBS routes. The picker
//! page (T40) answers 404 until then.

mod obs;
pub mod remote;
pub use obs::SceneHub;
pub use remote::RemoteHub;

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
    /// The scenes of a browser-source output: the current one first (if
    /// there is one), then each change. `None` if the output has no
    /// browser-source URL. [`SceneHub::watch`] does this.
    fn watch_scene(&self, output: OutputId) -> Option<std::sync::mpsc::Receiver<Arc<Scene>>>;
    fn blob_path(&self, hash: &BlobHash) -> Option<std::path::PathBuf>;
    /// The renderer that rasterises deck slides for browser sources
    /// (`GET /frames`). Without one, decks are drawn in the DOM from the
    /// scene instead.
    fn renderer(&self) -> Option<Arc<lampstand_render::Renderer>> {
        None
    }
    /// The sync node that takes `WS /sync` connections; `None` answers 404.
    fn sync(&self) -> Option<Arc<SyncNode>> {
        None
    }
    /// Lampstand Remote's pairing and sessions; `None` answers 404 on
    /// every `/remote` route.
    fn remotes(&self) -> Option<Arc<RemoteHub>> {
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
            .merge(obs::router(backend.clone()))
            .merge(remote::router(backend.clone()))
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
