//! Image textures: decoded once per blob, premultiplied, mipmapped, cached.
//!
//! T27 adds video textures beside these; both are drawn by the quad
//! pipeline, so a video node is an image node whose texture changes.

use std::collections::HashMap;

use lampstand_core::geom::Rect;
use lampstand_core::id::BlobHash;
use lampstand_core::scene::Fit;
use wgpu::util::DeviceExt;

use crate::gpu::{Gpu, Pipelines, QuadUniform, QuadVertex, Texture};
use crate::BlobSource;

pub(crate) struct CachedImage {
    pub texture: Texture,
    /// Size of the original image, before any downscale to fit the GPU.
    pub width: u32,
    pub height: u32,
}

/// Blob → texture, or `None` for a blob that is missing or failed to
/// decode (remembered, so it is not retried every frame).
#[derive(Default)]
pub(crate) struct ImageCache {
    entries: HashMap<BlobHash, Option<CachedImage>>,
}

impl ImageCache {
    pub fn get(&self, blob: &BlobHash) -> Option<&CachedImage> {
        self.entries.get(blob).and_then(Option::as_ref)
    }

    /// Loads `blob` if it is not cached yet; returns its size.
    pub fn load(
        &mut self,
        gpu: &Gpu,
        pipelines: &Pipelines,
        blobs: &dyn BlobSource,
        blob: &BlobHash,
    ) -> Option<(u32, u32)> {
        self.entries
            .entry(*blob)
            .or_insert_with(|| decode(gpu, pipelines, blobs, blob))
            .as_ref()
            .map(|image| (image.width, image.height))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Forgets every image not in `keep` (the output frame loop calls this
    /// as scenes change).
    pub fn retain(&mut self, keep: impl Fn(&BlobHash) -> bool) {
        self.entries.retain(|blob, _| keep(blob));
    }
}

fn decode(
    gpu: &Gpu,
    pipelines: &Pipelines,
    blobs: &dyn BlobSource,
    blob: &BlobHash,
) -> Option<CachedImage> {
    let bytes = std::fs::read(blobs.path(blob)?).ok()?;
    let image = image::load_from_memory(&bytes).ok()?;
    let (width, height) = (image.width(), image.height());
    if width == 0 || height == 0 {
        return None;
    }
    let max = gpu.max_texture_size();
    let image = if width > max || height > max {
        image.resize(max, max, image::imageops::FilterType::Triangle)
    } else {
        image
    };
    let mut rgba = image.into_rgba8();
    for px in rgba.pixels_mut() {
        let a = px[3] as u32;
        for c in &mut px.0[..3] {
            *c = ((*c as u32 * a + 127) / 255) as u8;
        }
    }
    let (w, h) = rgba.dimensions();
    let mips = 32 - w.max(h).leading_zeros();
    let texture = Texture::new(&gpu.device, "image", w, h, 1, mips);
    gpu.queue.write_texture(
        texture.texture.as_image_copy(),
        &rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * w),
            rows_per_image: Some(h),
        },
        texture.texture.size(),
    );
    generate_mips(gpu, pipelines, &texture.texture, mips);
    Some(CachedImage {
        texture,
        width,
        height,
    })
}

/// Box-filters each mip level from the one above with the quad pipeline.
fn generate_mips(gpu: &Gpu, pipelines: &Pipelines, texture: &wgpu::Texture, mips: u32) {
    if mips <= 1 {
        return;
    }
    let device = &gpu.device;
    let uniform = QuadUniform {
        opacity: 1.0,
        ..Default::default()
    };
    let uniforms = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("mip uniform"),
        contents: bytemuck::bytes_of(&uniform),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("mip quad"),
        contents: bytemuck::cast_slice(&fullscreen_quad()),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let level = |i| {
        texture.create_view(&wgpu::TextureViewDescriptor {
            base_mip_level: i,
            mip_level_count: Some(1),
            ..Default::default()
        })
    };
    let mut encoder = device.create_command_encoder(&Default::default());
    for i in 1..mips {
        let (src, dst) = (level(i - 1), level(i));
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipelines.quad_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &uniforms,
                        offset: 0,
                        size: wgpu::BufferSize::new(std::mem::size_of::<QuadUniform>() as u64),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&src),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&pipelines.linear_sampler),
                },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("mip"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &dst,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&pipelines.quad_single);
        pass.set_bind_group(0, &bind_group, &[0]);
        pass.set_vertex_buffer(0, vertices.slice(..));
        pass.draw(0..6, 0..1);
    }
    gpu.queue.submit([encoder.finish()]);
}

/// Two triangles covering clip space, with UVs.
pub(crate) fn fullscreen_quad() -> [QuadVertex; 6] {
    quad_vertices(
        [[-1.0, 1.0], [1.0, 1.0], [-1.0, -1.0], [1.0, -1.0]],
        [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
    )
}

/// Corners in the order top-left, top-right, bottom-left, bottom-right.
pub(crate) fn quad_vertices(positions: [[f32; 2]; 4], uvs: [[f32; 2]; 4]) -> [QuadVertex; 6] {
    [0, 1, 2, 2, 1, 3].map(|i| QuadVertex {
        position: positions[i],
        uv: uvs[i],
    })
}

/// Where an image lands inside `rect` (local space) and which part of the
/// texture it shows. `crop` is the PPTX source rectangle, as fractions of
/// the image; parts of it outside the image draw nothing.
pub(crate) fn place(
    rect: Rect,
    fit: Fit,
    crop: Option<Rect>,
    width: u32,
    height: u32,
) -> Option<(Rect, Rect)> {
    let crop = crop.unwrap_or(Rect {
        x: 0.0,
        y: 0.0,
        width: 1.0,
        height: 1.0,
    });
    if crop.width <= 0.0 || crop.height <= 0.0 || rect.width <= 0.0 || rect.height <= 0.0 {
        return None;
    }
    let (sw, sh) = (width as f32 * crop.width, height as f32 * crop.height);
    let (mut dest, mut uv) = (rect, crop);
    match fit {
        Fit::Fill => {}
        Fit::Contain => {
            let scale = (rect.width / sw).min(rect.height / sh);
            let (w, h) = (sw * scale, sh * scale);
            dest = Rect {
                x: rect.x + (rect.width - w) / 2.0,
                y: rect.y + (rect.height - h) / 2.0,
                width: w,
                height: h,
            };
        }
        Fit::Cover => {
            let scale = (rect.width / sw).max(rect.height / sh);
            let (fw, fh) = (rect.width / (sw * scale), rect.height / (sh * scale));
            uv = Rect {
                x: crop.x + crop.width * (1.0 - fw) / 2.0,
                y: crop.y + crop.height * (1.0 - fh) / 2.0,
                width: crop.width * fw,
                height: crop.height * fh,
            };
        }
    }
    // Clip the UV rect to the image, shrinking `dest` to match.
    let (x0, x1) = clip(uv.x, uv.width, &mut dest.x, &mut dest.width)?;
    let (y0, y1) = clip(uv.y, uv.height, &mut dest.y, &mut dest.height)?;
    Some((
        dest,
        Rect {
            x: x0,
            y: y0,
            width: x1 - x0,
            height: y1 - y0,
        },
    ))
}

fn clip(start: f32, len: f32, dest_start: &mut f32, dest_len: &mut f32) -> Option<(f32, f32)> {
    let (a, b) = (start.max(0.0), (start + len).min(1.0));
    if b <= a {
        return None;
    }
    let per_uv = *dest_len / len;
    *dest_start += (a - start) * per_uv;
    *dest_len = (b - a) * per_uv;
    Some((a, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    const R: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 200.0,
        height: 100.0,
    };

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn contain_letterboxes() {
        let (dest, uv) = place(R, Fit::Contain, None, 100, 100).unwrap();
        assert_eq!(dest, rect(50.0, 0.0, 100.0, 100.0));
        assert_eq!(uv, rect(0.0, 0.0, 1.0, 1.0));
    }

    #[test]
    fn cover_crops_edges() {
        let (dest, uv) = place(R, Fit::Cover, None, 100, 100).unwrap();
        assert_eq!(dest, R);
        assert_eq!(uv, rect(0.0, 0.25, 1.0, 0.5));
    }

    #[test]
    fn crop_selects_part_of_the_image() {
        let crop = rect(0.5, 0.0, 0.5, 1.0);
        let (dest, uv) = place(R, Fit::Fill, Some(crop), 100, 100).unwrap();
        assert_eq!(dest, R);
        assert_eq!(uv, crop);
    }

    #[test]
    fn crop_beyond_the_image_leaves_a_gap() {
        // srcRect with a negative left: the left quarter of the rect is empty.
        let crop = rect(-0.25, 0.0, 1.0, 1.0);
        let (dest, uv) = place(R, Fit::Fill, Some(crop), 100, 100).unwrap();
        assert_eq!(dest, rect(50.0, 0.0, 150.0, 100.0));
        assert_eq!(uv, rect(0.0, 0.0, 0.75, 1.0));
    }
}
