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
//!
//! # GPU architecture (T24)
//!
//! **Device.** One `wgpu::Device` per `Renderer`, on the integrated GPU
//! where there are two, else any GPU, else a software adapter (lavapipe,
//! WARP); `RenderError::NoAdapter` if there is none. Output surfaces are
//! created from the same `Instance` and checked with
//! `Surface::get_capabilities` against the chosen adapter.
//!
//! **Colour.** Every internal texture is `Rgba8Unorm` holding
//! premultiplied, sRGB-encoded values, and blending happens on those
//! values. That is what browsers do, so this renderer and the OBS browser
//! source (DOM) blend alike. glyphon runs in `ColorMode::Web` for the same
//! reason. A surface whose format is `*Srgb` needs its view created without
//! the `Srgb` suffix (or a decode in the blit), or text and gradients come
//! out too light.
//!
//! **Frame structure.** Drawing a scene is two halves:
//!
//! 1. `frame` (CPU) walks the scene and produces a `Frame`: a list of
//!    layers, each a painter's-order list of meshes, textured quads and
//!    glyphon text jobs. Paths are tessellated with lyon in local space;
//!    text is shaped with cosmic-text exactly as `lampstand-layout`
//!    measures it (see `text`). Nodes that cannot be drawn straight into
//!    their parent (group opacity, shadows, rotated text) get a layer.
//! 2. `encode` (GPU) uploads the frame and records one 4× MSAA pass per
//!    layer, children first, each resolved into a pooled texture; blurred
//!    layers get two more passes.
//!
//! A scene always renders into a plain `Rgba8Unorm` texture. Offscreen
//! rendering reads that texture back. Outputs keep one such texture per
//! scene on screen and composite them onto the surface (see "Outputs"
//! below). Build animations change node visibility, opacity and transforms
//! while the CPU half walks the tree, so they need no GPU changes.
//!
//! **Pipelines.** Three, all on `Rgba8Unorm`:
//!
//! - `shape`: lyon meshes. Vertices carry clip-space and local positions;
//!   the paint (solid, or a linear or radial gradient evaluated on the
//!   local position) comes from a dynamic-offset uniform. Gradients are
//!   rows of a 256-texel ramp texture built per frame.
//! - `quad`: a textured quad, for images, layer composites, mip generation
//!   and shadows (tinted by the texture's alpha). T27's video frames are a
//!   new texture `Source` for the same pipeline.
//! - `blur`: one direction of a separable Gaussian.
//!
//! plus glyphon's own pipeline, and the outputs' composite pipelines: the
//! `quad` shader built per surface format, with "over" and additive
//! blending.
//!
//! **Atlases and caches.** One glyphon `TextAtlas` (glyph bitmaps, trimmed
//! after each frame) and one `SwashCache` (glyph outlines, for text
//! outlines). Images are decoded once per blob, premultiplied, mipmapped
//! and kept by `BlobHash`. Layer and MSAA textures are pooled by size.
//!
//! **Text.** glyphon draws glyphs from the shared `Fonts`. Outlines are the
//! glyph outlines from swash, stroked by lyon at twice the width and drawn
//! under the fill. Underline and strike use the font's own metrics. Text
//! under a transform glyphon cannot draw (rotation, skew, mirror,
//! non-uniform scale) is drawn upright into a layer at the output scale
//! and composited through the transform.
//!
//! **Not yet.** Meshes are rebuilt every time a scene is drawn; outputs
//! draw a scene only when it or its animation changes, so this has not
//! mattered. Blurs wider than about 30 px (sigma) are clamped.
//!
//! # Outputs (T25)
//!
//! **One render thread.** `Outputs::spawn` moves a `Renderer` onto a
//! thread named `lampstand-render`, which owns the device and every
//! surface. Callers reach it only through an unbounded channel
//! (`OutputSink::send` and the `Outputs` methods), and it reports back
//! through a callback that must not block (`OutputEvent`). So the UI never
//! waits for a frame and the frame loop never waits for the UI. Offscreen
//! rendering elsewhere (thumbnails) uses its own `Renderer`, so it never
//! holds the mutex the frame loop needs.
//!
//! **Frame loop** (`output::engine`). Each pass drains the channel without
//! blocking, then draws every output that needs a frame and presents it,
//! FIFO, one surface after another. An output needs a frame when a command
//! changed it, a transition or build step is running, or its window has
//! not shown its first frame. When none does, the loop blocks on the
//! channel, so a static slide costs no GPU time; a command wakes it at
//! once. Frame latency is 1 by default (wgpu's
//! `desired_maximum_frame_latency`): on an idle output the frame for a
//! click is the next one scanned out.
//!
//! **Scenes are drawn once, then composited.** Each output keeps a texture
//! for the incoming scene, one for the outgoing scene during a transition,
//! one for the safe-area overlay and one for the calibration pattern. A
//! scene is drawn into its texture when it arrives and again only while a
//! build animation runs. Every frame is then a clear plus a few textured
//! quads, laid out by `output::timeline` (a cut is one quad, a fade two
//! with additive blending, so stream outputs cross-fade their transparency
//! correctly too). A new `Show` during a transition starts from the
//! incoming scene as last drawn; a new `Animate` finishes the running build
//! step first, as PowerPoint does.
//!
//! **Timelines are pure.** `output::timeline` turns (transition, time) into
//! composite layers and (build steps, time) into per-node `NodeAnim`s, so
//! all of it is unit-tested without a GPU. `Direction` always names a
//! side: what a push or cover enters from, where a wipe starts, where a
//! fly-in comes from and a fly-out goes.
//!
//! **Surfaces from any window.** `Outputs::attach` takes anything that
//! converts into `wgpu::SurfaceTarget<'static>`: an `Arc<winit::Window>`
//! in the demo and tests, a Tauri window in the app. The surface is created
//! on the caller's thread (macOS wants the main thread for that) and moved
//! to the render thread, which configures and presents. Surfaces prefer a
//! non-sRGB format, since scene textures already hold sRGB-encoded values.
//! winit is only a dev-dependency.
//!
//! **No flash.** Windows are created hidden, borderless, sized to their
//! display and with a black background. On the first frame the render
//! thread sends `OutputEvent::FirstFrame` and the window is shown, made
//! fullscreen and the cursor hidden. Windows and X11 present to hidden
//! windows, so the frame is already there. macOS does not (wgpu reports a
//! hidden window as occluded), so there the window appears black and its
//! first frame follows within one refresh. On macOS "fullscreen" means
//! simple fullscreen, not a new Space.
//!
//! **Displays** (`output::display`). The window system lists monitors as
//! `DisplayInfo`s and `assign_displays` matches them to outputs' saved
//! `DisplayMatch`es: name, then size, then nearest position. A display that
//! matches on neither name nor size is never taken. A missing projector's
//! output waits, hidden, rather than covering the operator's screen.
//!
//! **Unplugging.** A surface error affects only its own output: timeouts
//! and occlusion retry 16 ms later, outdated surfaces are reconfigured, and
//! a lost surface reports `SurfaceLost` and stops drawing while the other
//! outputs carry on. The output's scene is kept, so attaching a new window
//! when the display returns shows the live slide straight away.
//!
//! **Panics.** Each output's frame runs under `catch_unwind`. A panic
//! resets that output's textures, finishes its transition and build steps,
//! and draws its last scene again (`OutputEvent::Restarted`). If the same
//! output panics again within 10 s, its scene is dropped and it shows
//! black instead of crashing in a loop. A panic outside any one output's
//! frame resets all of them; three within 10 s blank all of them. The
//! default wgpu error handler (which panics) is replaced with one that
//! logs, so one bad frame costs a frame, not the thread.
//!
//! **Latency.** `OutputEvent::Presented` carries `FrameTiming` (when the
//! oldest command in the frame was sent, and when the frame was handed to
//! the display). With FIFO the frame reaches the screen at the next
//! refresh, so click-to-photon is at most send→present plus one refresh.
//! `tests/windows.rs` measures this on three real windows.
//!
//! **Video** (T27). `output::video::VideoClock` already tracks which video
//! nodes keep playing across scenes (same node ID and blob); T27's players
//! hang off it.

pub mod calibrate;
mod encode;
mod frame;
mod gpu;
mod images;
pub mod output;
mod paint;
mod tess;
mod text;

pub use calibrate::Edge;
pub use gpu::AdapterChoice;
pub use output::{Background, FrameTiming, OutputEvent, OutputOptions, Outputs};

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use lampstand_core::geom::Transform;
use lampstand_core::id::BlobHash;
use lampstand_layout::Fonts;

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

/// TS-1: numbered rings at 1% steps in from every edge, corner markers and
/// the current margins (see `calibrate`). The volunteer reads off the
/// lowest number visible on each edge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationPattern {
    /// The margins being adjusted, drawn as a bright outline.
    pub current: SafeArea,
    /// Numbers run 0 to `max_percent` from each edge inward.
    pub max_percent: u8,
    /// The edge being adjusted, drawn as a thick lime line with its label enlarged.
    #[serde(default)]
    pub active: Option<Edge>,
    /// Names the output in the pattern's caption, next to its aspect ratio.
    #[serde(default)]
    pub label: String,
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

/// How to set up a `Renderer`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RendererOptions {
    pub adapter: AdapterChoice,
    /// Load the system's fonts as well as the bundled ones. Off for golden
    /// tests, so results do not depend on the machine.
    pub system_fonts: bool,
}

impl Default for RendererOptions {
    fn default() -> Self {
        Self {
            adapter: AdapterChoice::PreferIntegrated,
            system_fonts: true,
        }
    }
}

/// Which adapter a renderer ended up on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterInfo {
    pub name: String,
    pub backend: String,
    pub software: bool,
}

pub struct Renderer {
    gpu: gpu::Gpu,
    pipelines: gpu::Pipelines,
    blobs: Arc<dyn BlobSource>,
    state: Mutex<State>,
    /// Held by `render_offscreen` from drawing to read-back: `draw` releases
    /// `state` before the encoder is submitted, and a second scene prepared
    /// in between would overwrite the shared glyph buffers.
    offscreen: Mutex<()>,
}

/// Everything drawing mutates. Behind a mutex so `render_offscreen` can
/// take `&self`; the render thread is its only real user.
struct State {
    fonts: Fonts,
    glyphs: encode::Glyphs,
    images: images::ImageCache,
    pool: encode::TexturePool,
}

impl Renderer {
    /// Creates the device. Prefers the integrated GPU on laptops with two
    /// (lower power, no switching stalls) unless configured otherwise;
    /// falls back to a software adapter so the app always starts.
    pub fn new(blobs: Arc<dyn BlobSource>) -> Result<Self, RenderError> {
        Self::with_options(blobs, RendererOptions::default())
    }

    pub fn with_options(
        blobs: Arc<dyn BlobSource>,
        options: RendererOptions,
    ) -> Result<Self, RenderError> {
        let gpu = gpu::Gpu::new(options.adapter)?;
        let pipelines = gpu::Pipelines::new(&gpu.device);
        let fonts = if options.system_fonts {
            Fonts::load()
        } else {
            Fonts::bundled_only()
        };
        let glyphs = encode::Glyphs::new(&gpu);
        Ok(Self {
            gpu,
            pipelines,
            blobs,
            offscreen: Mutex::new(()),
            state: Mutex::new(State {
                fonts,
                glyphs,
                images: images::ImageCache::default(),
                pool: encode::TexturePool::default(),
            }),
        })
    }

    pub fn adapter_info(&self) -> AdapterInfo {
        let info = self.gpu.adapter.get_info();
        AdapterInfo {
            name: info.name,
            backend: info.backend.to_string(),
            software: info.device_type == wgpu::DeviceType::Cpu,
        }
    }

    /// Draws one scene to an image: thumbnails, browser-source deck frames,
    /// golden-image tests. The scene is stretched to `width` × `height`;
    /// pixels nothing covers are transparent.
    pub fn render_offscreen(
        &self,
        scene: &Scene,
        width: u32,
        height: u32,
    ) -> Result<Image, RenderError> {
        let max = self.gpu.max_texture_size();
        if width == 0 || height == 0 || width > max || height > max {
            return Err(RenderError::Device(format!(
                "image size {width}x{height} is outside 1..={max}"
            )));
        }
        let _one_at_a_time = self.offscreen.lock().unwrap_or_else(|e| e.into_inner());
        let device = &self.gpu.device;
        let target = gpu::Texture::new(device, "offscreen", width, height, 1, 1);
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut encoder = device.create_command_encoder(&Default::default());
        let drawn = self.draw(
            scene,
            width,
            height,
            &target.view,
            &mut encoder,
            &HashMap::new(),
        );
        let image = drawn.and_then(|()| self.read_back(&target, encoder));
        if let Some(error) = pollster::block_on(scope.pop()) {
            return Err(RenderError::Device(error.to_string()));
        }
        image
    }

    /// Submits `encoder` with a copy of `target` (a `FORMAT` texture) and
    /// returns its pixels, straight alpha.
    pub(crate) fn read_back(
        &self,
        target: &gpu::Texture,
        mut encoder: wgpu::CommandEncoder,
    ) -> Result<Image, RenderError> {
        let device = &self.gpu.device;
        let (width, height) = target.size();
        let row = (4 * width).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: row as u64 * height as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            target.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(height),
                },
            },
            target.texture.size(),
        );
        self.gpu.queue.submit([encoder.finish()]);

        let (tx, rx) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = tx.send(result);
            });
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .map_err(|e| RenderError::Device(e.to_string()))?;
        rx.recv()
            .map_err(|e| RenderError::Device(e.to_string()))?
            .map_err(|e| RenderError::Device(e.to_string()))?;
        let mapped = readback
            .slice(..)
            .get_mapped_range()
            .map_err(|e| RenderError::Device(e.to_string()))?;
        let mut pixels = Vec::with_capacity(4 * width as usize * height as usize);
        for y in 0..height as usize {
            let start = y * row as usize;
            pixels.extend_from_slice(&mapped[start..start + 4 * width as usize]);
        }
        drop(mapped);
        unpremultiply(&mut pixels);
        Ok(Image {
            width,
            height,
            pixels,
        })
    }

    /// Records `scene`, stretched to `width` × `height`, into `target`
    /// (an `Rgba8Unorm` view of that size), with build-animation state
    /// `anims` applied. The output frame loop calls this for each scene on
    /// screen.
    pub(crate) fn draw(
        &self,
        scene: &Scene,
        width: u32,
        height: u32,
        target: &wgpu::TextureView,
        encoder: &mut wgpu::CommandEncoder,
        anims: &HashMap<lampstand_core::scene::NodeId, output::timeline::NodeAnim>,
    ) -> Result<(), RenderError> {
        // A panic while drawing leaves nothing half-updated that matters:
        // caches only ever gain complete entries.
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let State {
            fonts,
            glyphs,
            images,
            pool,
        } = &mut *state;
        let mut builder = frame::Builder {
            frame: frame::Frame::default(),
            gpu: &self.gpu,
            pipelines: &self.pipelines,
            blobs: &*self.blobs,
            images,
            fonts,
            swash: &mut glyphs.swash,
            anims,
        };
        let root = builder.push_layer(width, height, None);
        let fit = if scene.width > 0.0 && scene.height > 0.0 {
            Transform::scale(width as f32 / scene.width, height as f32 / scene.height)
        } else {
            Transform::IDENTITY
        };
        for node in &scene.nodes {
            builder.node(root, node, fit, 1.0);
        }
        let built = builder.frame;
        let result = encode::encode(
            &self.gpu,
            &self.pipelines,
            images,
            glyphs,
            fonts,
            pool,
            built,
            target,
            encoder,
        );
        glyphs.atlas.trim();
        pool.trim(2);
        result
    }
}

impl Renderer {
    /// Once more than `budget` images are cached, forgets those `keep`
    /// rejects. Decoding is the slow part of showing a picture, so images
    /// stay cached until there are many.
    pub(crate) fn evict_images(&self, budget: usize, keep: impl Fn(&BlobHash) -> bool) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.images.len() > budget {
            state.images.retain(keep);
        }
    }
}

fn unpremultiply(pixels: &mut [u8]) {
    for px in pixels.as_chunks_mut::<4>().0 {
        let a = px[3] as u32;
        if a == 0 {
            px[..3].fill(0);
        } else if a < 255 {
            for c in &mut px[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderError {
    NoAdapter,
    Device(String),
    Surface(String),
}
