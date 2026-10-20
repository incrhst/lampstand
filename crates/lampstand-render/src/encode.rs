//! Frame → GPU: uploads a built `Frame` and encodes its passes.
//!
//! Per frame: one vertex, index and uniform buffer each for shapes and
//! quads, one ramp texture, one glyphon `TextRenderer` per text job. Layer
//! textures come from a pool kept across frames. Each layer is one MSAA
//! pass resolved into its texture, plus two blur passes if it has a blur.

use std::collections::HashMap;

use glyphon::{Resolution, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport};
use wgpu::util::DeviceExt;

use crate::frame::{Frame, Item, Source};
use crate::gpu::{BlurUniform, Gpu, Pipelines, Texture, SAMPLES, UNIFORM_ALIGN};
use crate::images::ImageCache;
use crate::paint::{finish_v, RAMP_WIDTH};
use crate::RenderError;

/// Textures reused across frames, by (width, height, samples).
#[derive(Default)]
pub(crate) struct TexturePool {
    free: HashMap<(u32, u32, u32), Vec<Texture>>,
}

impl TexturePool {
    fn take(&mut self, device: &wgpu::Device, width: u32, height: u32, samples: u32) -> Texture {
        self.free
            .get_mut(&(width, height, samples))
            .and_then(Vec::pop)
            .unwrap_or_else(|| Texture::new(device, "layer", width, height, samples, 1))
    }

    fn give(&mut self, texture: Texture) {
        let (w, h) = texture.size();
        let key = (w, h, texture.texture.sample_count());
        self.free.entry(key).or_default().push(texture);
    }

    /// Drops pooled textures beyond `keep` per size.
    pub fn trim(&mut self, keep: usize) {
        for list in self.free.values_mut() {
            list.truncate(keep);
        }
        self.free.retain(|_, list| !list.is_empty());
    }
}

/// glyphon state kept across frames.
pub(crate) struct Glyphs {
    pub cache: glyphon::Cache,
    pub atlas: TextAtlas,
    pub renderers: Vec<TextRenderer>,
    pub viewports: HashMap<(u32, u32), Viewport>,
    pub swash: cosmic_text::SwashCache,
}

impl Glyphs {
    pub fn new(gpu: &Gpu) -> Self {
        let cache = glyphon::Cache::new(&gpu.device);
        let atlas = TextAtlas::with_color_mode(
            &gpu.device,
            &gpu.queue,
            &cache,
            crate::gpu::FORMAT,
            glyphon::ColorMode::Web,
        );
        Self {
            cache,
            atlas,
            renderers: Vec::new(),
            viewports: HashMap::new(),
            swash: cosmic_text::SwashCache::new(),
        }
    }
}

fn padded<T: bytemuck::Pod>(items: &[T]) -> Vec<u8> {
    let mut out = Vec::with_capacity(items.len().max(1) * UNIFORM_ALIGN as usize);
    for item in items {
        out.extend_from_slice(bytemuck::bytes_of(item));
        out.resize(out.len().next_multiple_of(UNIFORM_ALIGN as usize), 0);
    }
    if out.is_empty() {
        out.resize(UNIFORM_ALIGN as usize, 0);
    }
    out
}

fn offset(index: u32) -> u32 {
    index * UNIFORM_ALIGN as u32
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn encode(
    gpu: &Gpu,
    pipelines: &Pipelines,
    images: &ImageCache,
    glyphs: &mut Glyphs,
    fonts: &mut lampstand_layout::Fonts,
    pool: &mut TexturePool,
    mut frame: Frame,
    target: &wgpu::TextureView,
    encoder: &mut wgpu::CommandEncoder,
) -> Result<(), RenderError> {
    let device = &gpu.device;
    let buffer = |label, contents: &[u8], usage| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents,
            usage,
        })
    };
    // Empty buffers are not allowed; pad with one zero element.
    let non_empty = |bytes: &[u8]| {
        if bytes.is_empty() {
            vec![0u8; 16]
        } else {
            bytes.to_vec()
        }
    };

    let rows = frame.ramps.rows();
    for paint in &mut frame.paints {
        if paint.kind != 0 {
            paint.ramp_v = finish_v(paint.ramp_v, rows);
        }
    }
    let vertices = buffer(
        "shape vertices",
        &non_empty(bytemuck::cast_slice(&frame.vertices)),
        wgpu::BufferUsages::VERTEX,
    );
    let indices = buffer(
        "shape indices",
        &non_empty(bytemuck::cast_slice(&frame.indices)),
        wgpu::BufferUsages::INDEX,
    );
    let quad_vertices = buffer(
        "quad vertices",
        &non_empty(bytemuck::cast_slice(&frame.quad_vertices)),
        wgpu::BufferUsages::VERTEX,
    );
    let paints = buffer(
        "paints",
        &padded(&frame.paints),
        wgpu::BufferUsages::UNIFORM,
    );
    let quads = buffer("quads", &padded(&frame.quads), wgpu::BufferUsages::UNIFORM);

    let ramp = {
        let rows = rows.max(1) as u32;
        let mut texels = frame.ramps.texels.clone();
        texels.resize(RAMP_WIDTH * rows as usize, [0; 4]);
        let texture = device.create_texture_with_data(
            &gpu.queue,
            &wgpu::TextureDescriptor {
                label: Some("gradient ramps"),
                size: wgpu::Extent3d {
                    width: RAMP_WIDTH as u32,
                    height: rows,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: crate::gpu::FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            bytemuck::cast_slice(&texels),
        );
        texture.create_view(&Default::default())
    };

    // Layer textures; layer 0 draws into `target`.
    let layer_textures: Vec<Option<Texture>> = frame
        .layers
        .iter()
        .enumerate()
        .map(|(i, l)| (i > 0).then(|| pool.take(device, l.width, l.height, 1)))
        .collect();
    let view_of =
        |i: usize| -> &wgpu::TextureView { layer_textures[i].as_ref().map_or(target, |t| &t.view) };

    let bind = |layout, uniforms: &wgpu::Buffer, size: usize, view: &wgpu::TextureView, sampler| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: uniforms,
                        offset: 0,
                        size: wgpu::BufferSize::new(size as u64),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        })
    };
    let shape_group = bind(
        &pipelines.shape_layout,
        &paints,
        std::mem::size_of::<crate::gpu::PaintUniform>(),
        &ramp,
        &pipelines.linear_sampler,
    );
    let quad_size = std::mem::size_of::<crate::gpu::QuadUniform>();
    let mut quad_groups: HashMap<usize, wgpu::BindGroup> = HashMap::new();
    for layer in &frame.layers {
        for item in &layer.items {
            let Item::Quad {
                source, uniform, ..
            } = item
            else {
                continue;
            };
            let group = match source {
                Source::Image(blob) => {
                    let Some(image) = images.get(blob) else {
                        continue;
                    };
                    bind(
                        &pipelines.quad_layout,
                        &quads,
                        quad_size,
                        &image.texture.view,
                        &pipelines.mip_sampler,
                    )
                }
                Source::Layer(i) => bind(
                    &pipelines.quad_layout,
                    &quads,
                    quad_size,
                    view_of(*i),
                    &pipelines.linear_sampler,
                ),
                Source::Video(i) => bind(
                    &pipelines.quad_layout,
                    &quads,
                    quad_size,
                    &frame.videos[*i],
                    &pipelines.linear_sampler,
                ),
            };
            quad_groups.insert(*uniform as usize, group);
        }
    }

    // Blur: per blurred layer, a horizontal pass into a scratch texture and
    // a vertical one back.
    let blurred: Vec<usize> = (0..frame.layers.len())
        .filter(|&i| frame.layers[i].blur.is_some())
        .collect();
    let blur_uniforms: Vec<BlurUniform> = blurred
        .iter()
        .flat_map(|&i| {
            let l = &frame.layers[i];
            let sigma = l.blur.unwrap_or(1.0);
            let radius = ((sigma * 3.0).ceil() as i32).clamp(1, 96);
            [
                BlurUniform {
                    step: [1.0 / l.width as f32, 0.0],
                    sigma,
                    radius,
                },
                BlurUniform {
                    step: [0.0, 1.0 / l.height as f32],
                    sigma,
                    radius,
                },
            ]
        })
        .collect();
    let blur_buffer = buffer(
        "blurs",
        &padded(&blur_uniforms),
        wgpu::BufferUsages::UNIFORM,
    );
    let scratch: Vec<Texture> = blurred
        .iter()
        .map(|&i| pool.take(device, frame.layers[i].width, frame.layers[i].height, 1))
        .collect();
    let blur_size = std::mem::size_of::<BlurUniform>();
    let blur_groups: Vec<[wgpu::BindGroup; 2]> = blurred
        .iter()
        .zip(&scratch)
        .map(|(&i, s)| {
            [
                bind(
                    &pipelines.blur_layout,
                    &blur_buffer,
                    blur_size,
                    view_of(i),
                    &pipelines.linear_sampler,
                ),
                bind(
                    &pipelines.blur_layout,
                    &blur_buffer,
                    blur_size,
                    &s.view,
                    &pipelines.linear_sampler,
                ),
            ]
        })
        .collect();

    // Text.
    while glyphs.renderers.len() < frame.texts.len() {
        let renderer = TextRenderer::new(
            &mut glyphs.atlas,
            device,
            wgpu::MultisampleState {
                count: SAMPLES,
                ..Default::default()
            },
            None,
        );
        glyphs.renderers.push(renderer);
    }
    for job in &frame.texts {
        let l = &frame.layers[job.layer];
        let viewport = glyphs
            .viewports
            .entry((l.width, l.height))
            .or_insert_with(|| Viewport::new(device, &glyphs.cache));
        viewport.update(
            &gpu.queue,
            Resolution {
                width: l.width,
                height: l.height,
            },
        );
    }
    for (job, renderer) in frame.texts.iter().zip(&mut glyphs.renderers) {
        let l = &frame.layers[job.layer];
        let areas = job.areas.iter().map(|a| TextArea {
            buffer: &frame.buffers[a.buffer],
            left: a.left,
            top: a.top,
            scale: a.scale,
            bounds: TextBounds {
                left: 0,
                top: 0,
                right: l.width as i32,
                bottom: l.height as i32,
            },
            default_color: glyphon::Color::rgb(255, 255, 255),
            custom_glyphs: &[],
        });
        renderer
            .prepare(
                device,
                &gpu.queue,
                fonts.system(),
                &mut glyphs.atlas,
                &glyphs.viewports[&(l.width, l.height)],
                areas,
                &mut glyphs.swash,
            )
            .map_err(|e| RenderError::Device(format!("text: {e}")))?;
    }

    // Passes, children first.
    let mut msaa: HashMap<(u32, u32), Texture> = HashMap::new();
    for (i, layer) in frame.layers.iter().enumerate().rev() {
        let ms = msaa
            .entry((layer.width, layer.height))
            .or_insert_with(|| pool.take(device, layer.width, layer.height, SAMPLES));
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("layer"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &ms.view,
                    depth_slice: None,
                    resolve_target: Some(view_of(i)),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                ..Default::default()
            });
            pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
            for item in &layer.items {
                match item {
                    Item::Mesh { indices, paint } => {
                        pass.set_pipeline(&pipelines.shape);
                        pass.set_bind_group(0, &shape_group, &[offset(*paint)]);
                        pass.set_vertex_buffer(0, vertices.slice(..));
                        pass.draw_indexed(indices.clone(), 0, 0..1);
                    }
                    Item::Quad { first, uniform, .. } => {
                        let Some(group) = quad_groups.get(&(*uniform as usize)) else {
                            continue;
                        };
                        pass.set_pipeline(&pipelines.quad_msaa);
                        pass.set_bind_group(0, group, &[offset(*uniform)]);
                        pass.set_vertex_buffer(0, quad_vertices.slice(..));
                        pass.draw(*first..*first + 6, 0..1);
                    }
                    Item::Text(job) => {
                        glyphs.renderers[*job]
                            .render(
                                &glyphs.atlas,
                                &glyphs.viewports[&(layer.width, layer.height)],
                                &mut pass,
                            )
                            .map_err(|e| RenderError::Device(format!("text: {e}")))?;
                    }
                }
            }
        }
        if let Some(k) = blurred.iter().position(|&b| b == i) {
            for (direction, dst) in [&scratch[k].view, view_of(i)].into_iter().enumerate() {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("blur"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: dst,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
                pass.set_pipeline(&pipelines.blur);
                let index = (2 * k + direction) as u32;
                pass.set_bind_group(0, &blur_groups[k][direction], &[offset(index)]);
                pass.draw(0..3, 0..1);
            }
        }
    }

    // Textures go back to the pool; the encoder holds its own references.
    for texture in layer_textures
        .into_iter()
        .flatten()
        .chain(scratch)
        .chain(msaa.into_values())
    {
        pool.give(texture);
    }
    Ok(())
}
