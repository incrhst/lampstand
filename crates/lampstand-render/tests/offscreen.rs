//! Renders frames offscreen and checks the pixels: text stays inside the
//! safe area (TS-2), black and blank screens (LV-2), and the safe-area
//! overlay (TS-8). Skipped when the machine has no GPU adapter.

use lampstand_core::safe_area::{Rect, SafeArea};
use lampstand_render::live::{Live, Slide};
use lampstand_render::renderer::{Renderer, BACKGROUND, OVERLAY};

const WIDTH: u32 = 640;
const HEIGHT: u32 = 360;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

struct Frame {
    pixels: Vec<[u8; 3]>,
}

impl Frame {
    fn at(&self, x: u32, y: u32) -> [u8; 3] {
        self.pixels[(y * WIDTH + x) as usize]
    }

    /// The first pixel outside `rect` that is not plain background.
    fn stray(&self, rect: Rect) -> Option<(u32, u32, [u8; 3])> {
        (0..HEIGHT)
            .flat_map(|y| (0..WIDTH).map(move |x| (x, y)))
            .filter(|&(x, y)| !contains(rect, x, y))
            .map(|(x, y)| (x, y, self.at(x, y)))
            .find(|&(_, _, p)| !close(p, BACKGROUND))
    }
}

fn close(a: [u8; 3], b: [u8; 3]) -> bool {
    a.iter().zip(b).all(|(&a, b)| a.abs_diff(b) <= 2)
}

fn contains(rect: Rect, x: u32, y: u32) -> bool {
    let (x, y) = (x as f32 + 0.5, y as f32 + 0.5);
    x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height
}

fn renderer(safe_area: SafeArea) -> Option<Renderer> {
    let instance = wgpu::Instance::default();
    let Ok(adapter) =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
    else {
        eprintln!("no GPU adapter: skipping offscreen render test");
        return None;
    };
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()?;
    Some(Renderer::new(
        device, queue, FORMAT, WIDTH, HEIGHT, safe_area,
    ))
}

fn draw(renderer: &mut Renderer, live: &Live) -> Frame {
    let device = renderer.device().clone();
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    renderer.layout(live);
    renderer.draw(&texture.create_view(&Default::default()), live);

    // WIDTH * 4 is a multiple of 256, as buffer copies require.
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (WIDTH * HEIGHT * 4) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(WIDTH * 4),
                rows_per_image: Some(HEIGHT),
            },
        },
        texture.size(),
    );
    renderer.queue().submit(Some(encoder.finish()));
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, |result| result.expect("map readback"));
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("poll");
    let pixels = readback
        .slice(..)
        .get_mapped_range()
        .expect("mapped range")
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&[r, g, b, _]| [r, g, b])
        .collect();
    Frame { pixels }
}

fn live(text: &str) -> Live {
    Live::new(vec![Slide {
        label: "V1".into(),
        text: text.into(),
    }])
}

const VERSE: &str = "Amazing grace! How sweet the sound\nThat saved a wretch like me!\nI once was lost, but now am found;\nWas blind, but now I see.";

#[test]
fn text_stays_inside_the_safe_area() {
    let safe = SafeArea {
        top: 0.1,
        right: 0.1,
        bottom: 0.3,
        left: 0.1,
    };
    let Some(mut renderer) = renderer(safe) else {
        return;
    };
    let rect = renderer.safe_rect();
    let frame = draw(&mut renderer, &live(VERSE));

    assert_eq!(frame.stray(rect), None);
    let lit = frame
        .pixels
        .iter()
        .filter(|p| p.iter().all(|&c| c > 200))
        .count();
    assert!(
        lit > 500,
        "expected white text inside the safe area, found {lit} bright pixels"
    );
}

#[test]
fn overflowing_text_is_flagged_and_clipped() {
    let safe = SafeArea {
        top: 0.4,
        right: 0.3,
        bottom: 0.4,
        left: 0.3,
    };
    let Some(mut renderer) = renderer(safe) else {
        return;
    };
    let rect = renderer.safe_rect();
    let long = [VERSE; 4].join("\n");
    let frame = draw(&mut renderer, &live(&long));

    assert!(renderer.fit().unwrap().overflows);
    assert_eq!(frame.stray(rect), None);
}

#[test]
fn black_screen_is_all_black() {
    let Some(mut renderer) = renderer(SafeArea::default()) else {
        return;
    };
    renderer.show_safe_area = true;
    let mut live = live(VERSE);
    live.toggle_black();
    let frame = draw(&mut renderer, &live);
    assert!(frame.pixels.iter().all(|&p| p == [0, 0, 0]));
}

#[test]
fn blank_screen_keeps_the_background_without_text() {
    let Some(mut renderer) = renderer(SafeArea::default()) else {
        return;
    };
    let mut live = live(VERSE);
    live.toggle_blank();
    let frame = draw(&mut renderer, &live);
    assert!(frame.pixels.iter().all(|&p| close(p, BACKGROUND)));
}

#[test]
fn overlay_traces_the_safe_area() {
    let safe = SafeArea {
        top: 0.1,
        right: 0.1,
        bottom: 0.1,
        left: 0.1,
    };
    let Some(mut renderer) = renderer(safe) else {
        return;
    };
    renderer.show_safe_area = true;
    let rect = renderer.safe_rect();
    let mut live = live(VERSE);
    live.toggle_blank();
    let frame = draw(&mut renderer, &live);

    let (left, top) = (rect.x as u32, rect.y as u32);
    assert!(close(frame.at(left + 1, HEIGHT / 2), OVERLAY));
    assert!(close(frame.at(WIDTH / 2, top + 1), OVERLAY));
    assert!(close(frame.at(left - 2, HEIGHT / 2), BACKGROUND));
    assert!(close(frame.at(WIDTH / 2, HEIGHT / 2), BACKGROUND));
}
