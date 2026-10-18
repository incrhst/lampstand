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
//! WARP); `RenderError::NoAdapter` if there is none. T25 creates surfaces
//! from the same `Instance` and checks `Surface::get_capabilities` against
//! the chosen adapter.
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
//! rendering reads that texture back. T25's outputs keep one such texture
//! per scene on screen and composite them onto the surface: a cut is one
//! quad, a fade two, a push or wipe two with offsets or UV clips. Build
//! animations (T25) change node transforms and opacity before the CPU
//! half runs, so they need no GPU changes.
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
//! plus glyphon's own pipeline. T25 adds one `quad` variant per surface
//! format for the final composite.
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
//! **Not yet.** Meshes are rebuilt every frame; T25 may cache them per node
//! if profiling asks. Blurs wider than about 30 px (sigma) are clamped.

mod encode;
mod frame;
mod gpu;
mod images;
mod paint;
mod tess;
mod text;

pub use gpu::AdapterChoice;

use std::sync::{Arc, Mutex};

use lampstand_core::geom::Transform;
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
        let device = &self.gpu.device;
        let target = gpu::Texture::new(device, "offscreen", width, height, 1, 1);
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut encoder = device.create_command_encoder(&Default::default());
        self.draw(scene, width, height, &target.view, &mut encoder)?;

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
        if let Some(error) = pollster::block_on(scope.pop()) {
            return Err(RenderError::Device(error.to_string()));
        }

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
    /// (an `Rgba8Unorm` view of that size). T25's frame loop calls this for
    /// each scene on screen.
    fn draw(
        &self,
        scene: &Scene,
        width: u32,
        height: u32,
        target: &wgpu::TextureView,
        encoder: &mut wgpu::CommandEncoder,
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
