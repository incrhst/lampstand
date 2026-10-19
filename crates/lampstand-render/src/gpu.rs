//! The device, adapter choice and the crate's pipelines.

use std::num::NonZeroU64;

use crate::RenderError;

/// Format of every texture the renderer draws into: scene targets, layers,
/// images. Values are sRGB-encoded but the format is not `*Srgb`, so blending
/// happens on the encoded values, as browsers do. That keeps the wgpu output
/// and the OBS browser source (DOM) alike.
pub(crate) const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// MSAA sample count for every pass that draws geometry. lyon meshes have
/// hard edges, so this is the anti-aliasing. 4 is supported for `FORMAT` on
/// every backend.
pub(crate) const SAMPLES: u32 = 4;

/// Dynamic-offset alignment used for every per-draw uniform. 256 is the
/// largest `min_uniform_buffer_offset_alignment` any backend asks for.
pub(crate) const UNIFORM_ALIGN: u64 = 256;

/// Adapter preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AdapterChoice {
    /// The integrated GPU where there are two (lower power, no switching
    /// stalls), else whatever exists, else a software adapter.
    #[default]
    PreferIntegrated,
    /// The discrete GPU where there are two, else as above.
    PreferDiscrete,
    /// Only a software adapter (lavapipe, WARP). Used by golden tests.
    SoftwareOnly,
}

pub(crate) struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl Gpu {
    pub fn new(choice: AdapterChoice) -> Result<Self, RenderError> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter =
            pollster::block_on(pick_adapter(&instance, choice)).ok_or(RenderError::NoAdapter)?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("lampstand"),
            required_features: wgpu::Features::empty(),
            // Downlevel defaults run on lavapipe, WARP and old integrated
            // GPUs, but cap textures at 2048; ask for what the adapter has.
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            ..Default::default()
        }))
        .map_err(|e| RenderError::Device(e.to_string()))?;
        Ok(Self {
            instance,
            adapter,
            device,
            queue,
        })
    }

    pub fn max_texture_size(&self) -> u32 {
        self.device.limits().max_texture_dimension_2d
    }
}

async fn pick_adapter(instance: &wgpu::Instance, choice: AdapterChoice) -> Option<wgpu::Adapter> {
    let request = |power_preference, force_fallback_adapter| {
        instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference,
            force_fallback_adapter,
            compatible_surface: None,
            apply_limit_buckets: false,
        })
    };
    if choice != AdapterChoice::SoftwareOnly {
        let power = match choice {
            AdapterChoice::PreferDiscrete => wgpu::PowerPreference::HighPerformance,
            _ => wgpu::PowerPreference::LowPower,
        };
        if let Ok(adapter) = request(power, false).await {
            return Some(adapter);
        }
    }
    if let Ok(adapter) = request(wgpu::PowerPreference::None, true).await {
        return Some(adapter);
    }
    // Some drivers report a software adapter (lavapipe) as a CPU device but
    // not as a fallback; take it when asked for software only.
    if choice == AdapterChoice::SoftwareOnly {
        return instance
            .enumerate_adapters(wgpu::Backends::all())
            .await
            .into_iter()
            .find(|a| a.get_info().device_type == wgpu::DeviceType::Cpu);
    }
    None
}

/// The pipelines. Layers and scene targets all use `FORMAT`, so one set
/// serves every target. Output surfaces get their own composite pipelines
/// (`output::composite`), built per surface format from the same shader.
pub(crate) struct Pipelines {
    pub shape_layout: wgpu::BindGroupLayout,
    pub quad_layout: wgpu::BindGroupLayout,
    pub blur_layout: wgpu::BindGroupLayout,
    pub shape: wgpu::RenderPipeline,
    pub quad_msaa: wgpu::RenderPipeline,
    pub quad_single: wgpu::RenderPipeline,
    pub blur: wgpu::RenderPipeline,
    pub linear_sampler: wgpu::Sampler,
    pub mip_sampler: wgpu::Sampler,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct ShapeVertex {
    pub position: [f32; 2],
    pub local: [f32; 2],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct QuadVertex {
    pub position: [f32; 2],
    pub uv: [f32; 2],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct PaintUniform {
    pub kind: u32,
    pub ramp_v: f32,
    pub opacity: f32,
    pub _pad: f32,
    pub color: [f32; 4],
    pub geom: [f32; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct QuadUniform {
    pub mode: u32,
    pub opacity: f32,
    pub _pad: [f32; 2],
    pub tint: [f32; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct BlurUniform {
    pub step: [f32; 2],
    pub sigma: f32,
    pub radius: i32,
}

/// Premultiplied alpha over.
const BLEND: wgpu::BlendState = wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING;

impl Pipelines {
    pub fn new(device: &wgpu::Device) -> Self {
        let uniform_entry = |size: usize| wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: true,
                min_binding_size: NonZeroU64::new(size as u64),
            },
            count: None,
        };
        let texture_entries = [
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ];
        let layout = |label, size| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(label),
                entries: &[uniform_entry(size), texture_entries[0], texture_entries[1]],
            })
        };
        let shape_layout = layout("shape", std::mem::size_of::<PaintUniform>());
        let quad_layout = layout("quad", std::mem::size_of::<QuadUniform>());
        let blur_layout = layout("blur", std::mem::size_of::<BlurUniform>());

        let shape_module = device.create_shader_module(wgpu::include_wgsl!("shaders/shape.wgsl"));
        let quad_module = device.create_shader_module(wgpu::include_wgsl!("shaders/quad.wgsl"));
        let blur_module = device.create_shader_module(wgpu::include_wgsl!("shaders/blur.wgsl"));

        let pipeline_layout = |bind_group_layout: &wgpu::BindGroupLayout| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[Some(bind_group_layout)],
                immediate_size: 0,
            })
        };
        let vertex_attributes = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2];
        let vertex_buffers = [Some(wgpu::VertexBufferLayout {
            array_stride: 16,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &vertex_attributes,
        })];
        let pipeline = |label,
                        module: &wgpu::ShaderModule,
                        layout: &wgpu::PipelineLayout,
                        buffers: &[Option<wgpu::VertexBufferLayout>],
                        samples: u32,
                        blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers,
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState {
                    count: samples,
                    ..Default::default()
                },
                fragment: Some(wgpu::FragmentState {
                    module,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: FORMAT,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let shape_pl = pipeline_layout(&shape_layout);
        let quad_pl = pipeline_layout(&quad_layout);
        let blur_pl = pipeline_layout(&blur_layout);
        let shape = pipeline(
            "shape",
            &shape_module,
            &shape_pl,
            &vertex_buffers,
            SAMPLES,
            Some(BLEND),
        );
        let quad_msaa = pipeline(
            "quad msaa",
            &quad_module,
            &quad_pl,
            &vertex_buffers,
            SAMPLES,
            Some(BLEND),
        );
        let quad_single = pipeline(
            "quad",
            &quad_module,
            &quad_pl,
            &vertex_buffers,
            1,
            Some(BLEND),
        );
        let blur = pipeline("blur", &blur_module, &blur_pl, &[], 1, None);

        let sampler = |mipmap_filter| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter,
                ..Default::default()
            })
        };
        Self {
            shape_layout,
            quad_layout,
            blur_layout,
            shape,
            quad_msaa,
            quad_single,
            blur,
            linear_sampler: sampler(wgpu::MipmapFilterMode::Nearest),
            mip_sampler: sampler(wgpu::MipmapFilterMode::Linear),
        }
    }
}

/// A texture with its view.
pub(crate) struct Texture {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
}

impl Texture {
    pub fn new(
        device: &wgpu::Device,
        label: &str,
        width: u32,
        height: u32,
        samples: u32,
        mips: u32,
    ) -> Self {
        let mut usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
        if samples == 1 {
            usage |= wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST;
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: mips,
            sample_count: samples,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        Self { texture, view }
    }

    pub fn size(&self) -> (u32, u32) {
        (self.texture.width(), self.texture.height())
    }
}
