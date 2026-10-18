//! State → frame → pixels, with the headless renderer. Skips without a GPU
//! adapter unless LAMPSTAND_REQUIRE_GPU=1.

use std::sync::Arc;

use lampstand_output::frame::{Frame, THEME_BACKGROUND};
use lampstand_render::{Gpu, HeadlessRenderer, Image, Output, SafeArea};
use lampstand_show::protocol::{CueView, Mode, Snapshot};

const W: u32 = 640;
const H: u32 = 360;

fn renderer() -> Option<HeadlessRenderer> {
    match Gpu::headless() {
        Ok(gpu) => Some(HeadlessRenderer::new(gpu)),
        Err(e) if std::env::var("LAMPSTAND_REQUIRE_GPU").as_deref() != Ok("1") => {
            eprintln!("skipping: no GPU adapter ({e})");
            None
        }
        Err(e) => panic!("no GPU adapter: {e}"),
    }
}

fn output() -> Output {
    Output::new(W, H).with_safe_area(SafeArea { top: 0.1, right: 0.1, bottom: 0.1, left: 0.1 })
}

fn snapshot(mode: Mode, live_cue: Option<CueView>) -> Snapshot {
    Snapshot {
        service_name: "Test".into(),
        items: Vec::new(),
        preview: None,
        live: None,
        mode,
        preview_cue: None,
        live_cue,
        next_cue: None,
        logo: Some("/media/logo.png".into()),
        service_started_at: None,
        item_started_at: None,
        revision: 1,
    }
}

fn text() -> CueView {
    CueView::Text { lines: vec!["Amazing grace".into()], footer: Some("John Newton".into()), label: "V1".into(), notes: None }
}

fn rgb(img: &image::RgbaImage, x: u32, y: u32) -> [u8; 3] {
    let p = img.get_pixel(x, y).0;
    [p[0], p[1], p[2]]
}

fn near(a: [u8; 3], b: [u8; 3]) -> bool {
    a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 2)
}

const THEME: [u8; 3] = [THEME_BACKGROUND.r, THEME_BACKGROUND.g, THEME_BACKGROUND.b];

#[test]
fn black_and_nothing_live_are_black() {
    let Some(mut r) = renderer() else { return };
    for s in [snapshot(Mode::Black, Some(text())), snapshot(Mode::Normal, None)] {
        let (img, _) = r.render(&output(), &Frame::from_snapshot(&s).slide(None)).unwrap();
        assert!(img.pixels().all(|p| p.0[..3] == [0, 0, 0]), "{:?}", s.mode);
    }
}

#[test]
fn clear_is_the_background_only() {
    let Some(mut r) = renderer() else { return };
    let (img, _) = r.render(&output(), &Frame::from_snapshot(&snapshot(Mode::Clear, Some(text()))).slide(None)).unwrap();
    assert!(img.pixels().all(|p| near([p.0[0], p.0[1], p.0[2]], THEME)));
}

#[test]
fn text_stays_inside_the_safe_area() {
    let Some(mut r) = renderer() else { return };
    let (img, layout) = r.render(&output(), &Frame::from_snapshot(&snapshot(Mode::Normal, Some(text()))).slide(None)).unwrap();
    assert!(!layout.overflow());
    let safe = output().safe_area.rect(W as f32, H as f32);
    let mut text_pixels = 0;
    for (x, y, p) in img.enumerate_pixels() {
        let px = [p.0[0], p.0[1], p.0[2]];
        if near(px, THEME) {
            continue;
        }
        let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
        assert!(
            fx >= safe.x && fx <= safe.x + safe.width && fy >= safe.y && fy <= safe.y + safe.height,
            "text pixel {px:?} at ({x}, {y}) outside the safe area"
        );
        text_pixels += 1;
    }
    assert!(text_pixels > 200, "text was drawn");
}

#[test]
fn images_are_fitted_into_the_safe_area() {
    let Some(mut r) = renderer() else { return };
    let red = Arc::new(Image::new(16, 9, [255, 0, 0, 255].repeat(16 * 9)));
    let s = snapshot(Mode::Normal, Some(CueView::Image { url: "/x.png".into(), aspect: 16.0 / 9.0, label: "S".into(), notes: None }));
    let frame = Frame::from_snapshot(&s);
    assert_eq!(frame.image_url(), Some("/x.png"));
    let (img, _) = r.render(&output(), &frame.slide(Some(red.clone()))).unwrap();
    assert_eq!(rgb(&img, W / 2, H / 2), [255, 0, 0]);
    assert_eq!(rgb(&img, 2, 2), [0, 0, 0], "black outside the safe area");
    assert_eq!(rgb(&img, W / 2, 5), [0, 0, 0]);

    // The logo the same way.
    let s = snapshot(Mode::Logo, Some(text()));
    let (img, _) = r.render(&output(), &Frame::from_snapshot(&s).slide(Some(red))).unwrap();
    assert_eq!(rgb(&img, W / 2, H / 2), [255, 0, 0]);
    assert_eq!(rgb(&img, 2, 2), [0, 0, 0]);
}
