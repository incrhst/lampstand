//! Video and audio playback for backgrounds, media items and video embedded
//! in decks.
//!
//! PRD: LV-4, PS-3. NDI, Spout and Syphon (OB-3) are Phase 2.
//!
//! The real backend is GStreamer (`gstreamer` feature: `playbin3` into an
//! `appsink` producing RGBA frames). Without the feature, `NullBackend`
//! reports every file as unsupported so the rest of the app builds and
//! tests without GStreamer installed.

use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayOptions {
    pub looped: bool,
    pub muted: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    /// RGBA8, straight alpha.
    pub pixels: Arc<[u8]>,
    pub pts_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerState {
    Loading,
    Playing,
    Paused,
    Ended,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaError {
    Unsupported(String),
    Backend(String),
}

pub trait MediaBackend: Send + Sync {
    fn open(&self, path: &Path, options: PlayOptions) -> Result<Box<dyn Player>, MediaError>;
}

pub trait Player: Send {
    fn play(&mut self);
    fn pause(&mut self);
    fn seek(&mut self, ms: u64);
    fn state(&self) -> PlayerState;
    fn duration_ms(&self) -> Option<u64>;
    /// The newest decoded frame not yet taken, if any. Called by the render
    /// thread once per display frame; never blocks.
    fn take_frame(&mut self) -> Option<VideoFrame>;
}

/// Used when GStreamer is not compiled in.
pub struct NullBackend;

impl MediaBackend for NullBackend {
    fn open(&self, path: &Path, _: PlayOptions) -> Result<Box<dyn Player>, MediaError> {
        Err(MediaError::Unsupported(path.display().to_string()))
    }
}
