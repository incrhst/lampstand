//! Draws one output's frame into any texture view: background, the live
//! slide's text inside the safe area, and an optional safe-area overlay.
//! The window (see `output`) and offscreen tests share it.
//!
//! PRD: LV-2, TS-2, TS-7, TS-8.

use glyphon::cosmic_text::Align;
use glyphon::{
    Attrs, Buffer, Cache, Color, Family, FontSystem, Metrics, Resolution, Shaping, SwashCache,
    TextArea, TextAtlas, TextBounds, TextRenderer, Viewport,
};
use lampstand_core::safe_area::{Rect, SafeArea};
use wgpu::util::DeviceExt;

use crate::fit::{fit_size, Fit};
use crate::live::{Live, Screen};

/// Background behind the lyrics, as sRGB.
pub const BACKGROUND: [u8; 3] = [16, 24, 40];
/// Colour of the safe-area overlay, as sRGB.
pub const OVERLAY: [u8; 3] = [245, 158, 11];
/// Largest and smallest text, as fractions of the output height.
const MAX_TEXT: f32 = 0.12;
const MIN_TEXT: f32 = 0.03;
const LINE_HEIGHT: f32 = 1.2;

pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    width: u32,
    height: u32,

    font_system: FontSystem,
    swash_cache: SwashCache,
    viewport: Viewport,
    atlas: TextAtlas,
    text_renderer: TextRenderer,
    buffer: Buffer,
    text_top: f32,
    fit: Option<Fit>,

    quads: QuadPipeline,
    pub safe_area: SafeArea,
    pub show_safe_area: bool,
}

impl Renderer {
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
        safe_area: SafeArea,
    ) -> Self {
        let mut font_system = FontSystem::new();
        let cache = Cache::new(&device);
        let viewport = Viewport::new(&device, &cache);
        let mut atlas = TextAtlas::new(&device, &queue, &cache, format);
        let text_renderer =
            TextRenderer::new(&mut atlas, &device, wgpu::MultisampleState::default(), None);
        let buffer = Buffer::new(&mut font_system, Metrics::new(48.0, 48.0 * LINE_HEIGHT));
        let quads = QuadPipeline::new(&device, format);
        Renderer {
            device,
            queue,
            width: width.max(1),
            height: height.max(1),
            font_system,
            swash_cache: SwashCache::new(),
            viewport,
            atlas,
            text_renderer,
            buffer,
            text_top: 0.0,
            fit: None,
            quads,
            safe_area,
            show_safe_area: false,
        }
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width.max(1);
        self.height = height.max(1);
    }

    /// The text box on this output, in physical pixels.
    pub fn safe_rect(&self) -> Rect {
        self.safe_area.rect(self.width as f32, self.height as f32)
    }

    /// The size the current slide was laid out at, if any.
    pub fn fit(&self) -> Option<Fit> {
        self.fit
    }

    /// Lays out the live slide: the largest size that fits the safe area,
    /// centred within it. Call after the slide, safe area or size changes.
    pub fn layout(&mut self, live: &Live) {
        let Some(slide) = live.current() else {
            self.fit = None;
            return;
        };
        let rect = self.safe_rect();
        let height = self.height as f32;
        let (font_system, buffer) = (&mut self.font_system, &mut self.buffer);
        let mut set = |size: f32| {
            buffer.set_metrics_and_size(
                Metrics::new(size, size * LINE_HEIGHT),
                Some(rect.width),
                None,
            );
            buffer.set_text(
                &slide.text,
                &Attrs::new().family(Family::SansSerif),
                Shaping::Advanced,
                Some(Align::Center),
            );
            buffer.shape_until_scroll(font_system, false);
            measure(buffer)
        };
        let fit = fit_size(
            height * MIN_TEXT,
            (height * MAX_TEXT).max(height * MIN_TEXT),
            |size| {
                let (w, h) = set(size);
                w <= rect.width + 0.5 && h <= rect.height
            },
        );
        let (_, text_height) = set(fit.size);
        self.text_top = rect.y + ((rect.height - text_height) / 2.0).max(0.0);
        self.fit = Some(fit);
    }

    /// Draws the frame into `view`, which must match the size and format
    /// this renderer was set up with, and submits it.
    pub fn draw(&mut self, view: &wgpu::TextureView, live: &Live) {
        let rect = self.safe_rect();
        let screen = live.screen();
        self.viewport.update(
            &self.queue,
            Resolution {
                width: self.width,
                height: self.height,
            },
        );

        let text: Vec<TextArea> = match live.visible() {
            Some(_) => vec![TextArea {
                buffer: &self.buffer,
                left: rect.x,
                top: self.text_top,
                scale: 1.0,
                // Clip to the safe area: nothing leaves it, even overflowing text.
                bounds: TextBounds {
                    left: rect.x as i32,
                    top: rect.y as i32,
                    right: (rect.x + rect.width) as i32,
                    bottom: (rect.y + rect.height) as i32,
                },
                default_color: Color::rgb(255, 255, 255),
                custom_glyphs: &[],
            }],
            None => vec![],
        };
        self.text_renderer
            .prepare(
                &self.device,
                &self.queue,
                &mut self.font_system,
                &mut self.atlas,
                &self.viewport,
                text,
                &mut self.swash_cache,
            )
            .expect("prepare text");

        let overlay = if self.show_safe_area && screen != Screen::Black {
            outline(rect, 3.0, OVERLAY)
        } else {
            vec![]
        };
        self.quads
            .prepare(&self.device, &overlay, self.width, self.height);

        let background = match screen {
            Screen::Black => wgpu::Color::BLACK,
            Screen::Live | Screen::Blank => linear(BACKGROUND),
        };
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("output"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(background),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.text_renderer
                .render(&self.atlas, &self.viewport, &mut pass)
                .expect("render text");
            self.quads.render(&mut pass);
        }
        self.queue.submit(Some(encoder.finish()));
        self.atlas.trim();
    }
}

/// Width of the widest line and total height of the laid-out text.
fn measure(buffer: &Buffer) -> (f32, f32) {
    buffer
        .layout_runs()
        .fold((0.0_f32, 0.0_f32), |(w, h), run| {
            (w.max(run.line_w), h.max(run.line_top + run.line_height))
        })
}

/// An sRGB colour as the linear value a render pass clears to.
fn linear([r, g, b]: [u8; 3]) -> wgpu::Color {
    let c = |v: u8| {
        let v = v as f64 / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    wgpu::Color {
        r: c(r),
        g: c(g),
        b: c(b),
        a: 1.0,
    }
}

/// Four thin rectangles tracing the inside edge of `rect`.
fn outline(rect: Rect, width: f32, color: [u8; 3]) -> Vec<Quad> {
    let Rect {
        x,
        y,
        width: w,
        height: h,
    } = rect;
    [
        Rect {
            x,
            y,
            width: w,
            height: width,
        },
        Rect {
            x,
            y: y + h - width,
            width: w,
            height: width,
        },
        Rect {
            x,
            y,
            width,
            height: h,
        },
        Rect {
            x: x + w - width,
            y,
            width,
            height: h,
        },
    ]
    .into_iter()
    .map(|rect| Quad { rect, color })
    .collect()
}

struct Quad {
    rect: Rect,
    color: [u8; 3],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Vertex {
    position: [f32; 2],
    color: [f32; 4],
}

/// Draws solid rectangles given in pixels.
struct QuadPipeline {
    pipeline: wgpu::RenderPipeline,
    vertices: Option<(wgpu::Buffer, u32)>,
}

impl QuadPipeline {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("quads"),
            source: wgpu::ShaderSource::Wgsl(include_str!("quads.wgsl").into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor::default());
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("quads"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4],
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        QuadPipeline {
            pipeline,
            vertices: None,
        }
    }

    fn prepare(&mut self, device: &wgpu::Device, quads: &[Quad], width: u32, height: u32) {
        if quads.is_empty() {
            self.vertices = None;
            return;
        }
        let (sx, sy) = (2.0 / width as f32, 2.0 / height as f32);
        let vertices: Vec<Vertex> = quads
            .iter()
            .flat_map(|q| {
                let c = linear(q.color);
                let color = [c.r as f32, c.g as f32, c.b as f32, 1.0];
                let (l, t) = (q.rect.x * sx - 1.0, 1.0 - q.rect.y * sy);
                let (rr, bb) = (
                    (q.rect.x + q.rect.width) * sx - 1.0,
                    1.0 - (q.rect.y + q.rect.height) * sy,
                );
                [[l, t], [rr, t], [l, bb], [l, bb], [rr, t], [rr, bb]]
                    .map(|position| Vertex { position, color })
            })
            .collect();
        let bytes: Vec<u8> = vertices
            .iter()
            .flat_map(|v| {
                v.position
                    .iter()
                    .chain(&v.color)
                    .flat_map(|f| f.to_ne_bytes())
            })
            .collect();
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("quads"),
            contents: &bytes,
            usage: wgpu::BufferUsages::VERTEX,
        });
        self.vertices = Some((buffer, vertices.len() as u32));
    }

    fn render(&self, pass: &mut wgpu::RenderPass<'_>) {
        if let Some((buffer, count)) = &self.vertices {
            pass.set_pipeline(&self.pipeline);
            pass.set_vertex_buffer(0, buffer.slice(..));
            pass.draw(0..*count, 0..1);
        }
    }
}
