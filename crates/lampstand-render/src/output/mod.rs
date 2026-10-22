//! Output windows: the render thread, surfaces, transitions and builds
//! (T25).
//!
//! `Outputs` owns the render thread. Callers attach a surface per output
//! window, then send `OutputCommand`s through `OutputSink::send`; nothing
//! they do ever waits on a frame, and nothing the render thread does ever
//! waits on them. See the crate docs ("Outputs") for the design.

mod composite;
pub mod display;
mod engine;
pub mod overlay;
pub mod timeline;
pub mod video;

use std::sync::mpsc::{self, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use lampstand_core::output::OutputId;

use crate::{Image, OutputCommand, OutputSink, RenderError, Renderer};
use engine::{Engine, Msg};

pub use display::{assign_displays, match_display, DisplayInfo};

/// How the render thread presents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputOptions {
    /// Frames queued per surface (wgpu's `desired_maximum_frame_latency`).
    /// 1 gives the lowest click-to-photon latency; 2 tolerates a slow frame
    /// without a stutter. Presenting is always FIFO.
    pub frame_latency: u32,
}

impl Default for OutputOptions {
    fn default() -> Self {
        Self { frame_latency: 1 }
    }
}

/// What a window is cleared to under the scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Background {
    /// Black and opaque: projectors and stage displays.
    #[default]
    Opaque,
    /// Transparent, for a stream window captured with alpha. Falls back to
    /// opaque where the platform cannot composite a transparent surface.
    Transparent,
}

/// When a frame that carried new commands went out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameTiming {
    pub output: OutputId,
    /// Frames presented on this output so far, this one included.
    pub frame: u64,
    /// When the oldest command first shown in this frame was sent.
    pub sent: Instant,
    /// When the frame was handed to the display (after `present`). With
    /// FIFO it reaches the screen at the next refresh, so click-to-photon
    /// is at most `presented - sent` plus one refresh interval.
    pub presented: Instant,
}

impl FrameTiming {
    pub fn latency(&self) -> Duration {
        self.presented - self.sent
    }
}

/// Reported by the render thread. The callback runs on the render thread,
/// so it must hand the event on (a channel, an event-loop proxy) and
/// return at once.
#[derive(Debug, Clone, PartialEq)]
pub enum OutputEvent {
    /// The output's first frame is ready: show its window now. Windows are
    /// created hidden with a black background, so no desktop or white flash
    /// appears. Where the platform presents to hidden windows (Windows,
    /// X11) the frame is already on the surface; where it will not (macOS
    /// reports hidden windows as occluded) the frame is drawn and goes out
    /// within one refresh of the window appearing, over its black
    /// background.
    FirstFrame(OutputId),
    /// A frame showing new commands was presented.
    Presented(FrameTiming),
    /// The surface stopped working (its display went away, the window was
    /// destroyed). The output's scene is kept; attach a new surface to
    /// show it again. Other outputs are unaffected.
    SurfaceLost { output: OutputId, reason: String },
    /// The render thread caught a panic and redrew the last scene, on one
    /// output or (`output: None`) all of them. `blanked` means a scene that
    /// kept panicking was dropped and the output shows black.
    Restarted {
        output: Option<OutputId>,
        message: String,
        blanked: bool,
    },
}

/// The render thread and its outputs.
pub struct Outputs {
    tx: Sender<Msg>,
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    thread: Option<JoinHandle<()>>,
}

impl Outputs {
    /// Starts the render thread, which takes over `renderer`. Offscreen
    /// rendering elsewhere (thumbnails) should use its own `Renderer`, so
    /// it never holds a lock the frame loop needs.
    pub fn spawn(
        renderer: Renderer,
        options: OutputOptions,
        on_event: impl Fn(OutputEvent) + Send + 'static,
    ) -> Result<Self, RenderError> {
        let instance = renderer.gpu.instance.clone();
        let adapter = renderer.gpu.adapter.clone();
        renderer
            .gpu
            .device
            .on_uncaptured_error(std::sync::Arc::new(|error| {
                // wgpu's default handler panics; a validation error in one
                // frame should cost that frame, not the thread.
                eprintln!("lampstand-render: GPU error: {error}");
            }));
        let (tx, rx) = mpsc::channel();
        let engine = Engine::new(renderer, options, Box::new(on_event));
        let thread = std::thread::Builder::new()
            .name("lampstand-render".into())
            .spawn(move || engine::run(engine, rx))
            .map_err(|e| RenderError::Device(format!("cannot start the render thread: {e}")))?;
        Ok(Self {
            tx,
            instance,
            adapter,
            thread: Some(thread),
        })
    }

    /// Attaches a window to `output`. `window` is anything with raw window
    /// and display handles that can move to the render thread: an
    /// `Arc<winit::window::Window>`, a Tauri `WebviewWindow`. Call this on
    /// the thread that owns the window (macOS requires the main thread for
    /// creating the surface); presenting then happens on the render thread.
    ///
    /// Create the window hidden, and show it on `OutputEvent::FirstFrame`.
    /// `width` and `height` are the window's inner size in physical pixels.
    pub fn attach(
        &self,
        output: OutputId,
        window: impl Into<wgpu::SurfaceTarget<'static>>,
        width: u32,
        height: u32,
        background: Background,
    ) -> Result<(), RenderError> {
        let surface = self
            .instance
            .create_surface(window)
            .map_err(|e| RenderError::Surface(e.to_string()))?;
        if surface.get_capabilities(&self.adapter).formats.is_empty() {
            return Err(RenderError::Surface(
                "the chosen GPU cannot present to this window".into(),
            ));
        }
        self.post(Msg::Attach {
            output,
            surface: Some(surface),
            width,
            height,
            background,
        })
    }

    /// Attaches an output with no window: it renders to an offscreen
    /// texture that `snapshot` reads. Used by tests and anything that
    /// needs the composited picture without a display.
    pub fn attach_headless(
        &self,
        output: OutputId,
        width: u32,
        height: u32,
    ) -> Result<(), RenderError> {
        self.post(Msg::Attach {
            output,
            surface: None,
            width,
            height,
            background: Background::Opaque,
        })
    }

    /// Lets `output` play the sound of its videos. Off unless set; the
    /// shell sets it for rehearsal windows only.
    pub fn set_audible(&self, output: OutputId, audible: bool) {
        let _ = self.post(Msg::Audible { output, audible });
    }

    /// The window was resized (physical pixels). Any transition in progress
    /// on it jumps to its end.
    pub fn resize(&self, output: OutputId, width: u32, height: u32) {
        let _ = self.post(Msg::Resize {
            output,
            width,
            height,
        });
    }

    /// Drops `output`'s surface and waits (up to a second) until the render
    /// thread has let go of it, so the window can then be destroyed. The
    /// output's scene is kept for a later `attach`.
    pub fn detach(&self, output: OutputId) {
        let (done, wait) = mpsc::channel();
        if self.post(Msg::Detach { output, done }).is_ok() {
            let _ = wait.recv_timeout(Duration::from_secs(1));
        }
    }

    /// The output's current picture, composited as on screen. Blocks until
    /// the render thread has drawn it.
    pub fn snapshot(&self, output: OutputId) -> Result<Image, RenderError> {
        let (reply, wait) = mpsc::channel();
        self.post(Msg::Snapshot { output, reply })?;
        wait.recv_timeout(Duration::from_secs(10))
            .map_err(|_| RenderError::Device("the render thread did not answer".into()))?
    }

    /// Makes the render thread panic, on the next frame of `output` or (for
    /// `None`) at once, to exercise recovery.
    #[doc(hidden)]
    pub fn inject_panic(&self, output: Option<OutputId>) {
        let _ = self.post(Msg::InjectPanic(output));
    }

    fn post(&self, msg: Msg) -> Result<(), RenderError> {
        self.tx
            .send(msg)
            .map_err(|_| RenderError::Device("the render thread has stopped".into()))
    }
}

impl OutputSink for Outputs {
    fn send(&self, output: OutputId, command: OutputCommand) {
        let _ = self.post(Msg::Command {
            output,
            command,
            sent: Instant::now(),
        });
    }
}

impl Drop for Outputs {
    fn drop(&mut self) {
        // Closing the channel stops the loop.
        let (tx, _) = mpsc::channel();
        drop(std::mem::replace(&mut self.tx, tx));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
