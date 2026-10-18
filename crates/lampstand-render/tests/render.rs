//! Off-screen rendering checks. These need a GPU adapter (a software one such
//! as lavapipe or WARP is fine). Without one they are skipped, unless
//! LAMPSTAND_REQUIRE_GPU is set, as it should be on CI runners with an adapter.
//!
//! Rendered frames are written to `target/tmp/render-snapshots/` for review.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use image::RgbaImage;
use lampstand_render::calibration::{self, CORNERS};
use lampstand_render::{
    Background, BackgroundMode, Color, Gpu, HeadlessRenderer, Image, Output, Rect, SafeArea, Slide, TextSlide,
};

const VERSE: &str = "Amazing grace! How sweet the sound\nThat saved a wretch like me!\nI once was lost, but now am found;\nWas blind, but now I see.";
const BLUE: Color = Color::rgb(20, 40, 120);

fn renderer() -> Option<HeadlessRenderer> {
    match Gpu::headless() {
        Ok(gpu) => Some(HeadlessRenderer::new(gpu)),
        Err(e) if std::env::var_os("LAMPSTAND_REQUIRE_GPU").is_none() => {
            eprintln!("skipping GPU test: {e}");
            None
        }
        Err(e) => panic!("{e}"),
    }
}

fn save(image: &RgbaImage, name: &str) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("render-snapshots");
    std::fs::create_dir_all(&dir).unwrap();
    image.save(dir.join(name)).unwrap();
}

fn px(image: &RgbaImage, x: f32, y: f32) -> [u8; 4] {
    image.get_pixel(x as u32, y as u32).0
}

fn rgb(c: Color) -> [u8; 4] {
    [c.r, c.g, c.b, 255]
}

fn inside(r: Rect, x: u32, y: u32) -> bool {
    let (x, y) = (x as f32 + 0.5, y as f32 + 0.5);
    x >= r.x && x <= r.x + r.width && y >= r.y && y <= r.y + r.height
}

fn margins() -> SafeArea {
    SafeArea { top: 0.06, right: 0.1, bottom: 0.2, left: 0.04 }
}

/// Every pixel outside the safe area is exactly `outside`; returns how many
/// pixels inside it are bright (text).
fn check_text_confined(image: &RgbaImage, safe: Rect, outside: [u8; 4]) -> usize {
    let mut text = 0;
    for (x, y, p) in image.enumerate_pixels() {
        if inside(safe, x, y) {
            text += (p.0[0] > 200 && p.0[1] > 200 && p.0[2] > 200) as usize;
        } else {
            assert_eq!(p.0, outside, "pixel {x},{y} outside the safe area");
        }
    }
    text
}

#[test]
fn text_slide_stays_in_safe_area_with_bleed() {
    let Some(mut r) = renderer() else { return };
    for (name, w, h) in [("4x3", 800, 600), ("16x9", 960, 540), ("32x9", 1920, 540)] {
        let output = Output::new(w, h).with_safe_area(margins());
        let slide = Slide::text(TextSlide::new(VERSE).with_footer("John Newton")).with_background(Background::Color(BLUE));
        let (image, layout) = r.render(&output, &slide).unwrap();
        save(&image, &format!("text-bleed-{name}.png"));
        let text = check_text_confined(&image, layout.safe, rgb(BLUE));
        assert!(text > 1000, "{name}: expected visible text, got {text} bright pixels");
    }
}

#[test]
fn background_fit_leaves_black_outside_safe_area() {
    let Some(mut r) = renderer() else { return };
    let mut output = Output::new(960, 540).with_safe_area(margins());
    output.background_mode = BackgroundMode::Fit;
    let slide = Slide::text(TextSlide::new(VERSE)).with_background(Background::Color(BLUE));
    let (image, layout) = r.render(&output, &slide).unwrap();
    save(&image, "text-fit-16x9.png");
    check_text_confined(&image, layout.safe, [0, 0, 0, 255]);
    assert_eq!(px(&image, layout.safe.x + 2.0, layout.safe.y + 2.0), rgb(BLUE));
}

#[test]
fn overflowing_text_is_clipped_to_safe_area() {
    let Some(mut r) = renderer() else { return };
    let output = Output::new(640, 480).with_safe_area(margins());
    let body = VERSE.repeat(6);
    let (image, layout) = r.render(&output, &Slide::text(TextSlide::new(body)).with_background(Background::Color(BLUE))).unwrap();
    save(&image, "text-overflow-4x3.png");
    assert!(layout.overflow());
    check_text_confined(&image, layout.safe, rgb(BLUE));
}

#[test]
fn calibration_shows_corners_edges_and_safe_area() {
    let Some(mut r) = renderer() else { return };
    let output = Output::new(1280, 720).with_safe_area(SafeArea { top: 0.04, right: 0.06, bottom: 0.08, left: 0.02 });
    let (image, _) = r.render(&output, &Slide::calibration("Projector 1")).unwrap();
    save(&image, "calibration-16x9.png");
    let (w, h) = (1280.0, 720.0);
    // Corner markers, inside the 2-pixel red edge line.
    let corners = [(6.0, 12.0), (w - 7.0, 12.0), (w - 7.0, h - 13.0), (6.0, h - 13.0)];
    for ((x, y), color) in corners.into_iter().zip(CORNERS) {
        assert_eq!(px(&image, x, y), rgb(color), "corner at {x},{y}");
    }
    // The edge line runs along the very edge between the corners.
    assert_eq!(px(&image, w / 3.0, 0.0), rgb(calibration::EDGE));
    // The safe area outline, here 2% in from the left at mid-height.
    assert_eq!(px(&image, w * 0.02 + 1.0, h * 0.37), rgb(calibration::SAFE));
    // Ring numbers are drawn: the red "0" in the 0-2% band at the top centre.
    let band = (3..(h * 0.02) as u32).flat_map(|y| (620..660).map(move |x| (x, y)));
    assert!(band.filter(|&(x, y)| image.get_pixel(x, y).0[0] > 150).count() > 10);
}

#[test]
fn slide_image_is_letterboxed_into_safe_area() {
    let Some(mut r) = renderer() else { return };
    let red = Arc::new(Image::new(16, 9, [230, 20, 20, 255].repeat(16 * 9)));
    let output = Output::new(800, 600).with_safe_area(margins());
    let slide = Slide::image(red).with_background(Background::Color(BLUE));
    let (image, layout) = r.render(&output, &slide).unwrap();
    save(&image, "image-16x9-on-4x3.png");
    let rect = layout.image.unwrap();
    let c = (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
    assert_eq!(px(&image, c.0, c.1), [230, 20, 20, 255]);
    assert_eq!(px(&image, c.0, rect.y - 3.0), rgb(BLUE), "letterbox above the image");
    assert_eq!(px(&image, c.0, rect.y + rect.height + 3.0), rgb(BLUE), "letterbox below the image");
}

#[test]
fn background_image_covers_without_stretching() {
    let Some(mut r) = renderer() else { return };
    // Left half green, right half red; on a narrower screen the centre is cropped in.
    let mut rgba = Vec::new();
    for _y in 0..10 {
        for x in 0..40 {
            rgba.extend(if x < 20 { [0, 200, 0, 255] } else { [200, 0, 0, 255] });
        }
    }
    let output = Output::new(400, 300);
    let slide = Slide::default().with_background(Background::Image(Arc::new(Image::new(40, 10, rgba))));
    let (image, _) = r.render(&output, &slide).unwrap();
    assert_eq!(px(&image, 50.0, 150.0), [0, 200, 0, 255]);
    assert_eq!(px(&image, 350.0, 150.0), [200, 0, 0, 255]);
}

#[test]
fn changing_slide_is_fast() {
    let Some(mut r) = renderer() else { return };
    let output = Output::new(1920, 1080).with_safe_area(margins());
    r.render(&output, &Slide::text(TextSlide::new(VERSE))).unwrap();
    let renderer = r.renderer();
    let mut worst = std::time::Duration::ZERO;
    for verse in VERSE.lines() {
        // Best of three, so a busy machine's scheduling noise doesn't count.
        let best = (0..3)
            .map(|i| {
                let slide = Slide::text(TextSlide::new(format!("{} {i}", verse.repeat(3))));
                let start = Instant::now();
                renderer.prepare(&output, &slide).unwrap();
                start.elapsed()
            })
            .min()
            .unwrap();
        worst = worst.max(best);
    }
    eprintln!("slowest slide layout + upload: {worst:?}");
    // Generous for unoptimised test builds on shared CI; release is far lower.
    assert!(worst.as_millis() < 250, "{worst:?}");
}

#[test]
fn preview_overlay_outlines_safe_area() {
    let Some(mut r) = renderer() else { return };
    let mut output = Output::new(960, 540).with_safe_area(margins());
    output.show_safe_area = true;
    let (image, layout) = r.render(&output, &Slide::default()).unwrap();
    let s = layout.safe;
    let amber = rgb(lampstand_render::SAFE_AREA_OVERLAY);
    assert_eq!(px(&image, s.x + 1.0, s.y + s.height / 2.0), amber);
    assert_eq!(px(&image, s.x + s.width / 2.0, s.y + s.height - 1.5), amber);
    assert_eq!(px(&image, s.x - 2.0, s.y + s.height / 2.0), [0, 0, 0, 255]);
}
