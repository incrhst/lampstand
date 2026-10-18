//! Render engine: draws scenes with wgpu, into native output windows and
//! offscreen images.
//!
//! PRD: LV-3, TS-1, TS-8, OB-1; NFR latency and reliability.
//!
//! Threading: one render thread owns the wgpu device and every output
//! surface, and runs its own frame loop paced by the display (FIFO
//! present). The UI thread never blocks it, so a stalled UI cannot freeze
//! a projector. Other threads talk to it only through `OutputCommand`s,
//! which are serialisable so that outputs can move to a separate process
//! later without changing callers.
//!
//! Between two scenes the renderer runs the transition. Video nodes with
//! the same node ID and blob in both scenes keep playing instead of
//! restarting, so a motion background continues under changing lyrics.

use std::sync::Arc;

use lampstand_core::output::OutputId;
use lampstand_core::safe_area::SafeArea;
use lampstand_core::scene::{Animation, Scene, Transition};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum OutputCommand {
    /// Replace the scene, running the transition.
    Show {
        scene: Arc<Scene>,
        transition: Transition,
    },
    /// Play a build step's animations on the current scene.
    Animate(Vec<Animation>),
    /// Draw the safe-area outline over the scene (preview panes, TS-8).
    SafeAreaOverlay(Option<SafeArea>),
    /// Replace everything with the calibration pattern (TS-1), or return
    /// to the scene.
    Calibrate(Option<CalibrationPattern>),
}

/// TS-1: numbered edges and corner markers. The volunteer reads off the
/// lowest number visible on each edge; each number is 1% of the screen.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CalibrationPattern {
    /// The margins being adjusted, drawn as a bright outline.
    pub current: SafeArea,
    /// Numbers run 0 to `max_percent` from each edge inward.
    pub max_percent: u8,
}

/// Where commands for one output go.
pub trait OutputSink: Send + Sync {
    fn send(&self, output: OutputId, command: OutputCommand);
}

/// RGBA8, straight alpha, row-major.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// Looks up blob bytes (images, video files) for the renderer.
pub trait BlobSource: Send + Sync {
    fn path(&self, blob: &lampstand_core::id::BlobHash) -> Option<std::path::PathBuf>;
}

pub struct Renderer {
    _private: (),
}

impl Renderer {
    /// Creates the device. Prefers the integrated GPU on laptops with two
    /// (lower power, no switching stalls) unless configured otherwise;
    /// falls back to a software adapter so the app always starts.
    pub fn new(blobs: Arc<dyn BlobSource>) -> Result<Self, RenderError> {
        let _ = blobs;
        todo!("T24")
    }

    /// Draws one scene to an image: thumbnails, browser-source deck frames,
    /// golden-image tests.
    pub fn render_offscreen(
        &self,
        scene: &Scene,
        width: u32,
        height: u32,
    ) -> Result<Image, RenderError> {
        let _ = (scene, width, height);
        todo!("T24")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderError {
    NoAdapter,
    Device(String),
    Surface(String),
}
