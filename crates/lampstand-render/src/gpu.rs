//! wgpu device setup and the per-output renderer.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use glyphon::{Cache, Resolution, SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport};
use lampstand_core::safe_area::Rect;
use wgpu::util::DeviceExt;

use crate::frame::{self, Item};
use crate::layout::SlideLayout;
use crate::scene::{Color, Image, Output, Slide};
use crate::text::TextEngine;

#[derive(Debug)]
pub enum RenderError {
    /// No GPU adapter (not even a software one) is available.
    NoAdapter(String),
    Device(String),
    Surface(String),
    Readback(String),
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RenderError::NoAdapter(e) => write!(f, "no GPU adapter: {e}"),
            RenderError::Device(e) => write!(f, "GPU device: {e}"),
            RenderError::Surface(e) => write!(f, "output surface: {e}"),
            RenderError::Readback(e) => write!(f, "reading back frame: {e}"),
        }
    }
}

impl std::error::Error for RenderError {}

/// One GPU device shared by every output.
pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl Gpu {
    /// A device for rendering off-screen only (tests, thumbnails). Falls back
    /// to a software adapter when there is no GPU.
    pub fn headless() -> Result<Arc<Gpu>, RenderError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        Self::from_instance(instance, None)
    }

    /// Picks an adapter that can present to `compatible_surface`, if given,
    /// and opens a device asking only for WebGL2-level limits so it runs on
    /// old integrated graphics.
    pub fn from_instance(
        instance: wgpu::Instance,
        compatible_surface: Option<&wgpu::Surface<'_>>,
    ) -> Result<Arc<Gpu>, RenderError> {
        pollster::block_on(async {
            let mut options = wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::None,
                force_fallback_adapter: false,
                compatible_surface,
                ..Default::default()
            };
            let adapter = match instance.request_adapter(&options).await {
                Ok(adapter) => adapter,
                Err(_) => {
                    options.force_fallback_adapter = true;
                    instance.request_adapter(&options).await.map_err(|e| RenderError::NoAdapter(e.to_string()))?
                }
            };
            let limits = wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits());
            let (device, queue) = adapter
                .request_device(&wgpu::DeviceDescriptor {
                    label: Some("lampstand"),
                    required_limits: limits,
                    ..Default::default()
                })
                .await
                .map_err(|e| RenderError::Device(e.to_string()))?;
            Ok(Arc::new(Gpu { instance, adapter, device, queue }))
        })
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    pos: [f32; 2],
    color: [f32; 4],
    uv: [f32; 2],
}

const SHADER: &str = r#"
struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
};

@vertex
fn vs(@location(0) pos: vec2<f32>, @location(1) color: vec4<f32>, @location(2) uv: vec2<f32>) -> VOut {
    var out: VOut;
    out.pos = vec4<f32>(pos, 0.0, 1.0);
    out.color = color;
    out.uv = uv;
    return out;
}

@fragment
fn fs_solid(in: VOut) -> @location(0) vec4<f32> {
    return in.color;
}

@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

@fragment
fn fs_image(in: VOut) -> @location(0) vec4<f32> {
    return textureSample(tex, samp, in.uv) * in.color;
}
"#;

enum Draw {
    Solid(std::ops::Range<u32>),
    Image(std::ops::Range<u32>, usize),
}

struct CachedImage {
    // Holding the Arc keeps its address from being reused by another image.
    _image: Arc<Image>,
    bind_group: wgpu::BindGroup,
}

/// Draws slides for one output. Call [`Renderer::prepare`] when the slide or
/// output changes, then [`Renderer::render`] for each frame.
pub struct Renderer {
    gpu: Arc<Gpu>,
    format: wgpu::TextureFormat,
    solid: wgpu::RenderPipeline,
    image: wgpu::RenderPipeline,
    image_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    text: TextEngine,
    swash: SwashCache,
    atlas: TextAtlas,
    text_renderer: TextRenderer,
    viewport: Viewport,
    images: HashMap<usize, CachedImage>,
    vertices: Option<wgpu::Buffer>,
    draws: Vec<Draw>,
    layout: Option<SlideLayout>,
}

impl Renderer {
    /// `format` is the format of the textures this renderer draws into.
    pub fn new(gpu: Arc<Gpu>, format: wgpu::TextureFormat) -> Self {
        let device = &gpu.device;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("lampstand quads"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let image_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("slide image"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline = |entry: &str, layouts: &[Option<&wgpu::BindGroupLayout>]| {
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: layouts,
                immediate_size: 0,
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Vertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4, 2 => Float32x2],
                    })],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let solid = pipeline("fs_solid", &[]);
        let image = pipeline("fs_image", &[Some(&image_layout)]);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("slide image"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let cache = Cache::new(device);
        let viewport = Viewport::new(device, &cache);
        let mut atlas = TextAtlas::new(device, &gpu.queue, &cache, format);
        let text_renderer = TextRenderer::new(&mut atlas, device, wgpu::MultisampleState::default(), None);

        Renderer {
            format,
            solid,
            image,
            image_layout,
            sampler,
            text: TextEngine::new(),
            swash: SwashCache::new(),
            atlas,
            text_renderer,
            viewport,
            images: HashMap::new(),
            vertices: None,
            draws: Vec::new(),
            layout: None,
            gpu,
        }
    }

    pub fn gpu(&self) -> &Arc<Gpu> {
        &self.gpu
    }

    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// Fonts used by this output, e.g. to add a theme's font.
    pub fn text_engine(&mut self) -> &mut TextEngine {
        &mut self.text
    }

    /// The layout of the prepared slide, if any.
    pub fn layout(&self) -> Option<&SlideLayout> {
        self.layout.as_ref()
    }

    /// Lay out `slide` for `output` and upload what it needs. Returns the
    /// layout, including the overflow flag (TS-7).
    pub fn prepare(&mut self, output: &Output, slide: &Slide) -> Result<&SlideLayout, RenderError> {
        let frame = frame::build(&mut self.text, output, slide);
        let (w, h) = (output.width as f32, output.height as f32);
        let srgb = self.format.is_srgb();

        let mut vertices = Vec::new();
        let mut draws = Vec::new();
        let mut used = Vec::new();
        for item in &frame.items {
            let start = vertices.len() as u32;
            match item {
                Item::Solid { rect, color } => {
                    push_quad(&mut vertices, *rect, w, h, to_shader(*color, srgb), [0.0; 4]);
                    let end = vertices.len() as u32;
                    // Merge runs of solid quads into one draw.
                    if let Some(Draw::Solid(range)) = draws.last_mut() {
                        range.end = end;
                    } else {
                        draws.push(Draw::Solid(start..end));
                    }
                }
                Item::Image { rect, uv, image } => {
                    let key = self.upload(image);
                    used.push(key);
                    push_quad(&mut vertices, *rect, w, h, [1.0; 4], *uv);
                    draws.push(Draw::Image(start..vertices.len() as u32, key));
                }
            }
        }
        self.images.retain(|key, _| used.contains(key));
        self.vertices = (!vertices.is_empty()).then(|| {
            self.gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("slide quads"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            })
        });
        self.draws = draws;

        let gpu = &self.gpu;
        self.viewport.update(&gpu.queue, Resolution { width: output.width, height: output.height });
        let areas = frame.texts.iter().map(|t| TextArea {
            buffer: &t.buffer,
            left: t.left,
            top: t.top,
            scale: 1.0,
            bounds: bounds(t.clip),
            default_color: glyphon::Color::rgba(t.color.r, t.color.g, t.color.b, t.color.a),
            custom_glyphs: &[],
        });
        self.text_renderer
            .prepare(&gpu.device, &gpu.queue, &mut self.text.fonts, &mut self.atlas, &self.viewport, areas, &mut self.swash)
            .map_err(|e| RenderError::Device(e.to_string()))?;
        self.atlas.trim();

        Ok(self.layout.insert(frame.layout))
    }

    fn upload(&mut self, image: &Arc<Image>) -> usize {
        let key = Arc::as_ptr(image) as usize;
        if self.images.contains_key(&key) {
            return key;
        }
        let device = &self.gpu.device;
        let size = wgpu::Extent3d { width: image.width, height: image.height, depth_or_array_layers: 1 };
        let texture = device.create_texture_with_data(
            &self.gpu.queue,
            &wgpu::TextureDescriptor {
                label: Some("slide image"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &image.rgba,
        );
        let view = texture.create_view(&Default::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("slide image"),
            layout: &self.image_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        self.images.insert(key, CachedImage { _image: image.clone(), bind_group });
        key
    }

    /// Draw the prepared slide into `target`, clearing it to black first.
    pub fn render(&mut self, target: &wgpu::TextureView) -> Result<(), RenderError> {
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("slide"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if let Some(vertices) = &self.vertices {
                pass.set_vertex_buffer(0, vertices.slice(..));
                for draw in &self.draws {
                    match draw {
                        Draw::Solid(range) => {
                            pass.set_pipeline(&self.solid);
                            pass.draw(range.clone(), 0..1);
                        }
                        Draw::Image(range, key) => {
                            pass.set_pipeline(&self.image);
                            pass.set_bind_group(0, &self.images[key].bind_group, &[]);
                            pass.draw(range.clone(), 0..1);
                        }
                    }
                }
            }
            self.text_renderer
                .render(&self.atlas, &self.viewport, &mut pass)
                .map_err(|e| RenderError::Device(e.to_string()))?;
        }
        self.gpu.queue.submit(Some(encoder.finish()));
        Ok(())
    }
}

fn bounds(r: Rect) -> TextBounds {
    TextBounds {
        left: r.x.floor() as i32,
        top: r.y.floor() as i32,
        right: (r.x + r.width).ceil() as i32,
        bottom: (r.y + r.height).ceil() as i32,
    }
}

/// Vertex colour for the target: sRGB targets blend in linear light, so
/// convert; others take the sRGB values as they are.
fn to_shader(c: Color, srgb_target: bool) -> [f32; 4] {
    let f = |v: u8| {
        let v = v as f32 / 255.0;
        if !srgb_target {
            v
        } else if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    [f(c.r), f(c.g), f(c.b), c.a as f32 / 255.0]
}

fn push_quad(out: &mut Vec<Vertex>, r: Rect, w: f32, h: f32, color: [f32; 4], uv: [f32; 4]) {
    let x = |px: f32| px / w * 2.0 - 1.0;
    let y = |px: f32| 1.0 - px / h * 2.0;
    let (x0, y0, x1, y1) = (x(r.x), y(r.y), x(r.x + r.width), y(r.y + r.height));
    let [u0, v0, u1, v1] = uv;
    let v = |pos, uv| Vertex { pos, color, uv };
    out.extend([
        v([x0, y0], [u0, v0]),
        v([x0, y1], [u0, v1]),
        v([x1, y1], [u1, v1]),
        v([x0, y0], [u0, v0]),
        v([x1, y1], [u1, v1]),
        v([x1, y0], [u1, v0]),
    ]);
}
