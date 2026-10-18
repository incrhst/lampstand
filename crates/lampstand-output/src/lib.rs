//! The projector side of Lampstand: a separate process with one borderless
//! fullscreen wgpu window per output display (see `docs/v0.1.md`).
//!
//! It connects to the local server's WebSocket as `role: output`, draws the
//! live cue of every `state` it receives, and sends keys pressed in its
//! windows back as `command` messages. The pieces:
//!
//! - [`args`]: the command line.
//! - [`frame`]: what a snapshot puts on the projector ([`frame::Frame`]),
//!   and the render [`Slide`](lampstand_render::Slide) for it.
//! - [`images`]: fetching deck images and the logo over HTTP, and a small
//!   LRU of decoded images so going live is one texture upload.
//! - [`keys`]: keyboard to commands.
//! - [`net`]: the WebSocket client, with reconnect every 500 ms.
//! - [`calibration`]: per-display margins in `calibration.json`, and the
//!   calibration grid's edge adjustment.

pub mod args;
pub mod calibration;
pub mod frame;
pub mod images;
pub mod keys;
pub mod net;

use std::sync::Arc;

use lampstand_render::Image;
use lampstand_show::protocol::Snapshot;

/// Everything that reaches the window thread from elsewhere.
#[derive(Debug)]
pub enum Event {
    Connected,
    Disconnected,
    State(Box<Snapshot>),
    /// A fetch started by [`images::ImageCache`] finished.
    Image { key: images::ImageKey, result: Result<Arc<Image>, String> },
}
