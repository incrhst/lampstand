//! The last pass of a frame: finished scene textures onto the output.
//!
//! Each scene on screen is already a texture (see `engine`), so a frame is
//! a clear plus one textured quad per `CompositeLayer`: a cut is one quad,
//! a fade two. The quads use T24's `quad` shader, with one pipeline per
//! (target format, blend) built on first use; surfaces are usually
//! `Bgra8Unorm`, headless targets and snapshots `Rgba8Unorm`.

use std::collections::HashMap;

use wgpu::util::DeviceExt;

use super::timeline::{Blend, CompositeLayer};
use crate::gpu::{Gpu, Pipelines, QuadUniform, QuadVertex, UNIFORM_ALIGN};

pub(crate) struct Compositor {
    pipelines: HashMap<(wgpu::TextureFormat, Blend), wgpu::RenderPipeline>,
    module: wgpu::ShaderModule,
    layout: wgpu::PipelineLayout,
}

const ADD: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    },
};

impl Compositor {
    pub fn new(gpu: &Gpu, pipelines: &Pipelines) -> Self {
        let module = gpu
            .device
            .create_shader_module(wgpu::include_wgsl!("../shaders/quad.wgsl"));
        let layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("composite"),
                bind_group_layouts: &[Some(&pipelines.quad_layout)],
                immediate_size: 0,
            });
        Self {
            pipelines: HashMap::new(),
            module,
            layout,
        }
    }

    fn pipeline(
        &mut self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        blend: Blend,
    ) -> &wgpu::RenderPipeline {
        let Self {
            pipelines,
            module,
            layout,
        } = self;
        pipelines.entry((format, blend)).or_insert_with(|| {
            let attributes = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2];
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("composite"),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<QuadVertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &attributes,
                    })],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(match blend {
                            Blend::Over => wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING,
                            Blend::Add => ADD,
                        }),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        })
    }

    /// Clears `target` (a `width` × `height` view of `format`) to `clear`
    /// and draws `layers` over it, back to front.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        gpu: &Gpu,
        pipelines: &Pipelines,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        (width, height): (u32, u32),
        clear: wgpu::Color,
        layers: &[(&wgpu::TextureView, CompositeLayer)],
    ) {
        let device = &gpu.device;
        let (w, h) = (width as f32, height as f32);
        let mut vertices: Vec<QuadVertex> = Vec::with_capacity(6 * layers.len());
        let mut uniforms: Vec<u8> = Vec::new();
        let mut draws = Vec::new();
        for (view, layer) in layers {
            let c = layer.clip;
            if layer.opacity <= 0.0 || c.width <= 0.0 || c.height <= 0.0 {
                continue;
            }
            // The clip rect of the scene, in output pixels, after the
            // layer's scale (about the centre) and offset.
            let place = |u: f32, v: f32| {
                let x = (u * w - w / 2.0) * layer.scale + w / 2.0 + layer.offset.0 * w;
                let y = (v * h - h / 2.0) * layer.scale + h / 2.0 + layer.offset.1 * h;
                [x / w * 2.0 - 1.0, 1.0 - y / h * 2.0]
            };
            let uvs = [
                [c.x, c.y],
                [c.x + c.width, c.y],
                [c.x, c.y + c.height],
                [c.x + c.width, c.y + c.height],
            ];
            let positions = uvs.map(|[u, v]| place(u, v));
            vertices.extend(crate::images::quad_vertices(positions, uvs));
            let uniform = QuadUniform {
                mode: 0,
                opacity: layer.opacity.min(1.0),
                ..Default::default()
            };
            uniforms.extend_from_slice(bytemuck::bytes_of(&uniform));
            uniforms.resize(uniforms.len().next_multiple_of(UNIFORM_ALIGN as usize), 0);
            draws.push((*view, layer.blend));
        }

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("composite"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        if draws.is_empty() {
            return;
        }
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("composite vertices"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("composite uniforms"),
            contents: &uniforms,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        for (k, (view, blend)) in draws.into_iter().enumerate() {
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &pipelines.quad_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &uniform_buffer,
                            offset: 0,
                            size: wgpu::BufferSize::new(std::mem::size_of::<QuadUniform>() as u64),
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&pipelines.linear_sampler),
                    },
                ],
            });
            pass.set_pipeline(self.pipeline(device, format, blend));
            pass.set_bind_group(0, &group, &[k as u32 * UNIFORM_ALIGN as u32]);
            let first = 6 * k as u32;
            pass.draw(first..first + 6, 0..1);
        }
    }
}
