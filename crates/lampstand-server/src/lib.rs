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

use std::net::SocketAddr;
use std::sync::Arc;

use lampstand_core::id::BlobHash;
use lampstand_core::output::OutputId;
use lampstand_core::scene::Scene;

pub const DEFAULT_PORT: u16 = 7340;

/// What the server needs from the app.
pub trait Backend: Send + Sync + 'static {
    /// Checks a browser-source key.
    fn output_key_valid(&self, output: OutputId, key: &str) -> bool;
    /// The current scene for a browser-source output, and a way to wait for
    /// the next one.
    fn watch_scene(&self, output: OutputId) -> Option<std::sync::mpsc::Receiver<Arc<Scene>>>;
    fn blob_path(&self, hash: &BlobHash) -> Option<std::path::PathBuf>;
}

pub struct Server {
    _private: (),
}

impl Server {
    pub async fn start(addr: SocketAddr, backend: Arc<dyn Backend>) -> std::io::Result<Self> {
        let _ = (addr, backend);
        todo!("T42")
    }

    pub fn local_addr(&self) -> SocketAddr {
        todo!("T42")
    }
}
