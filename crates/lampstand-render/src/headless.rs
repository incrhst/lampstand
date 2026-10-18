//! Rendering to an off-screen texture and reading it back, for tests in CI
//! (no display needed), thumbnails and operator previews.

use std::sync::Arc;

use crate::gpu::{Gpu, RenderError, Renderer};
use crate::layout::SlideLayout;
use crate::scene::{Output, Slide};

pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

pub struct HeadlessRenderer {
    renderer: Renderer,
}

impl HeadlessRenderer {
    pub fn new(gpu: Arc<Gpu>) -> Self {
        HeadlessRenderer { renderer: Renderer::new(gpu, FORMAT) }
    }

    pub fn renderer(&mut self) -> &mut Renderer {
        &mut self.renderer
    }

    /// Render `slide` as `output` would show it.
    pub fn render(&mut self, output: &Output, slide: &Slide) -> Result<(image::RgbaImage, SlideLayout), RenderError> {
        let layout = self.renderer.prepare(output, slide)?.clone();
        let gpu = self.renderer.gpu().clone();
        let (width, height) = (output.width, output.height);
        let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("headless output"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        self.renderer.render(&texture.create_view(&Default::default()))?;

        let row = width * 4;
        let padded_row = row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("headless readback"),
            size: padded_row as u64 * height as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded_row), rows_per_image: Some(height) },
            },
            size,
        );
        gpu.queue.submit(Some(encoder.finish()));

        let (tx, rx) = std::sync::mpsc::channel();
        buffer.map_async(wgpu::MapMode::Read, .., move |r| {
            let _ = tx.send(r);
        });
        gpu.device
            .poll(wgpu::PollType::Wait { submission_index: None, timeout: None })
            .map_err(|e| RenderError::Readback(e.to_string()))?;
        rx.recv()
            .map_err(|e| RenderError::Readback(e.to_string()))?
            .map_err(|e| RenderError::Readback(e.to_string()))?;

        let mut pixels = Vec::with_capacity((row * height) as usize);
        {
            let mapped = buffer.get_mapped_range(..).map_err(|e| RenderError::Readback(format!("{e:?}")))?;
            for y in 0..height as usize {
                let start = y * padded_row as usize;
                pixels.extend_from_slice(&mapped[start..start + row as usize]);
            }
        }
        buffer.unmap();
        let image = image::RgbaImage::from_raw(width, height, pixels).expect("readback size");
        Ok((image, layout))
    }
}
