//! Safe-area layout and shrink-to-fit, on the CPU only (no GPU or display).

use std::sync::Arc;

use lampstand_render::layout::{contains, full_rect, safe_rect};
use lampstand_render::{
    layout_slide, BackgroundMode, Image, Output, Rect, SafeArea, Slide, SlideLayout, TextEngine, TextSlide,
};

const VERSE: &str = "Amazing grace! How sweet the sound\nThat saved a wretch like me!\nI once was lost, but now am found;\nWas blind, but now I see.";

const LONG: &str = "In the beginning was the Word, and the Word was with God, and the Word was God. The same was in the beginning with God. All things were made by him; and without him was not any thing made that was made. In him was life; and the life was the light of men. And the light shineth in darkness; and the darkness comprehended it not. There was a man sent from God, whose name was John. The same came for a witness, to bear witness of the Light, that all men through him might believe. He was not that Light, but was sent to bear witness of that Light. That was the true Light, which lighteth every man that cometh into the world.";

const ASPECTS: [(&str, u32, u32); 4] =
    [("4:3", 1024, 768), ("16:9", 1920, 1080), ("16:10", 1920, 1200), ("32:9 LED wall", 5760, 1620)];

fn margins() -> SafeArea {
    SafeArea { top: 0.05, right: 0.04, bottom: 0.12, left: 0.06 }
}

fn lay_out(output: &Output, text: TextSlide) -> SlideLayout {
    layout_slide(&mut TextEngine::new(), output, &Slide::text(text))
}

fn text_inside_safe_area(layout: &SlideLayout) {
    let text = layout.text.as_ref().expect("text layout");
    for line in text.lines.iter().chain(&text.footer) {
        assert!(contains(layout.safe, line.rect), "{line:?} leaves safe area {:?}", layout.safe);
    }
}

#[test]
fn snapshot_verse_on_16_9() {
    let output = Output::new(1920, 1080).with_safe_area(SafeArea { top: 0.05, right: 0.05, bottom: 0.05, left: 0.05 });
    let layout = lay_out(&output, TextSlide::new(VERSE).with_footer("John Newton, 1779"));
    let expected = "\
output 1920x1080
safe 96,54 1728x972
background 0,0 1920x1080
column 96,54 1728x972
size 97.20 overflow false
  104,275 1713x117 \"Amazing grace! How sweet the sound\"
  315,391 1290x117 \"That saved a wretch like me!\"
  164,508 1591x117 \"I once was lost, but now am found;\"
  392,625 1135x117 \"Was blind, but now I see.\"
  footer 792,981 335x45 \"John Newton, 1779\"
";
    assert_eq!(layout.to_string(), expected, "\n{layout}");
}

#[test]
fn snapshot_long_passage_overflows_on_4_3() {
    let output = Output::new(1024, 768).with_safe_area(margins());
    let layout = lay_out(&output, TextSlide::new(format!("{LONG} {LONG}")));
    let text = layout.text.as_ref().unwrap();
    assert!(layout.overflow());
    assert_eq!(text.font_size, output.text_style.min_size * 768.0);
    // Top-aligned when overflowing, so the start of the passage shows.
    assert_eq!(text.lines[0].rect.y.round(), safe_rect(&output).y.round());
    let expected_start = "\
output 1024x768
safe 61,38 922x637
background 0,0 1024x768
column 61,38 922x637
size 30.72 overflow true
";
    assert!(layout.to_string().starts_with(expected_start), "\n{layout}");
}

#[test]
fn short_text_uses_max_size_and_long_text_shrinks() {
    let output = Output::new(1920, 1080).with_safe_area(margins());
    let max = output.text_style.max_size * 1080.0;
    let min = output.text_style.min_size * 1080.0;

    let short = lay_out(&output, TextSlide::new("Amen"));
    assert_eq!(short.text.unwrap().font_size, max);

    let medium = lay_out(&output, TextSlide::new(&LONG[..300]));
    let size = medium.text.as_ref().unwrap().font_size;
    assert!(size < max && size > min, "size {size}");
    assert!(!medium.overflow());
    text_inside_safe_area(&medium);
}

#[test]
fn text_stays_in_safe_area_on_every_aspect() {
    for (name, w, h) in ASPECTS {
        for safe in [SafeArea::default(), margins(), SafeArea { top: 0.0, right: 0.2, bottom: 0.3, left: 0.0 }] {
            let output = Output::new(w, h).with_safe_area(safe);
            for text in [TextSlide::new(VERSE), TextSlide::new(&LONG[..400]).with_footer("John 1:1-5 (KJV)")] {
                let layout = lay_out(&output, text);
                assert!(!layout.overflow(), "{name} {safe:?}");
                text_inside_safe_area(&layout);
            }
        }
    }
}

#[test]
fn overflowing_text_is_flagged_at_min_size_on_every_aspect() {
    for (name, w, h) in ASPECTS {
        let output = Output::new(w, h).with_safe_area(margins());
        let layout = lay_out(&output, TextSlide::new(format!("{LONG} {LONG} {LONG}")));
        assert!(layout.overflow(), "{name}");
        assert_eq!(layout.text.unwrap().font_size, output.text_style.min_size * h as f32);
    }
}

#[test]
fn layout_adapts_to_aspect_ratio() {
    let column = |w, h| lay_out(&Output::new(w, h), TextSlide::new(VERSE)).text.unwrap().column;

    // 4:3, 16:9 and 16:10 use the whole safe area.
    for (_, w, h) in &ASPECTS[..3] {
        assert_eq!(column(*w, *h), full_rect(&Output::new(*w, *h)));
    }
    // An ultra-wide wall gets a centred column, not one stretched line.
    let wide = column(5760, 1620);
    assert_eq!(wide.width, 1620.0 * 2.4);
    assert_eq!(wide.x, (5760.0 - wide.width) / 2.0);

    // Same text, same height: line breaks differ with the width available.
    let narrow = lay_out(&Output::new(1024, 768), TextSlide::new(&LONG[..300]));
    let wide = lay_out(&Output::new(1365, 768), TextSlide::new(&LONG[..300]));
    assert!(narrow.text.unwrap().lines.len() > wide.text.unwrap().lines.len());
}

#[test]
fn background_bleeds_or_fits() {
    let mut output = Output::new(1920, 1080).with_safe_area(margins());
    let layout = layout_slide(&mut TextEngine::new(), &output, &Slide::default());
    assert_eq!(layout.background, full_rect(&output));

    output.background_mode = BackgroundMode::Fit;
    let layout = layout_slide(&mut TextEngine::new(), &output, &Slide::default());
    assert_eq!(layout.background, safe_rect(&output));
}

#[test]
fn slide_image_fits_safe_area_keeping_aspect() {
    let image = Arc::new(Image::new(16, 9, vec![255; 16 * 9 * 4]));
    for (name, w, h) in ASPECTS {
        let output = Output::new(w, h).with_safe_area(margins());
        let layout = layout_slide(&mut TextEngine::new(), &output, &Slide::image(image.clone()));
        let r: Rect = layout.image.unwrap();
        assert!(contains(layout.safe, r), "{name}");
        assert!((r.width / r.height - 16.0 / 9.0).abs() < 1e-3, "{name}");
        // Touches the safe area on two opposite sides.
        let s = layout.safe;
        assert!((r.width - s.width).abs() < 0.5 || (r.height - s.height).abs() < 0.5, "{name}");
    }
}

#[test]
fn lyric_lines_shrink_before_wrapping() {
    // At the maximum size the first line is too wide for this safe area.
    let output = Output::new(960, 540).with_safe_area(SafeArea { top: 0.06, right: 0.1, bottom: 0.2, left: 0.04 });
    let lyric = lay_out(&output, TextSlide::new(VERSE)).text.unwrap();
    assert_eq!(lyric.lines.len(), 4, "{:#?}", lyric.lines);
    assert!(lyric.font_size < output.text_style.max_size * 540.0);

    // A paragraph too long for one line wraps instead of dropping to minimum size.
    let passage = lay_out(&output, TextSlide::new(&LONG[..200])).text.unwrap();
    assert!(passage.lines.len() > 1);
    assert!(passage.font_size > output.text_style.min_size * 540.0);
}
