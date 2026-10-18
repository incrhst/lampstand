//! Renders sample slides off-screen to PNG, no display needed.
//!
//!     cargo run -p lampstand-render --example snapshot -- out-dir

use std::path::PathBuf;

use lampstand_render::{Background, Color, Gpu, HeadlessRenderer, Output, SafeArea, Slide, TextSlide};

fn main() {
    let dir = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "snapshots".into()));
    std::fs::create_dir_all(&dir).expect("create output dir");
    let mut renderer = HeadlessRenderer::new(Gpu::headless().expect("GPU"));
    let output = Output::new(1280, 720).with_safe_area(SafeArea { top: 0.04, right: 0.06, bottom: 0.1, left: 0.04 });
    let mut preview = output.clone();
    preview.show_safe_area = true;
    let lyric = TextSlide::new(
        "Amazing grace! How sweet the sound\nThat saved a wretch like me!\nI once was lost, but now am found;\nWas blind, but now I see.",
    )
    .with_footer("John Newton, 1779");
    let bg = Background::Color(Color::rgb(20, 40, 120));

    let shots = [
        ("calibration.png", &output, Slide::calibration("Projector 1 (left)")),
        ("text.png", &preview, Slide::text(lyric).with_background(bg)),
    ];
    for (name, output, slide) in shots {
        let (image, layout) = renderer.render(output, &slide).expect("render");
        image.save(dir.join(name)).expect("save");
        println!("{name}:\n{layout}");
    }
}
