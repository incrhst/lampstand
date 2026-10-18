//! Turns an output and a slide into a display list. This is the whole
//! layout step; the GPU code only uploads and draws what it is given.

use std::sync::Arc;

use lampstand_core::safe_area::Rect;

use crate::calibration::{self, outline};
use crate::layout::{background_rect, cover_uv, safe_rect, SlideLayout};
use crate::scene::{Background, Color, Content, Image, Output, Slide};
use crate::text::{PlacedText, TextEngine};

/// Colour of the safe-area overlay in preview panes (TS-8).
pub const SAFE_AREA_OVERLAY: Color = Color::rgb(255, 170, 0);

pub(crate) enum Item {
    Solid { rect: Rect, color: Color },
    Image { rect: Rect, uv: [f32; 4], image: Arc<Image> },
}

pub(crate) struct Frame {
    pub layout: SlideLayout,
    pub items: Vec<Item>,
    pub texts: Vec<PlacedText>,
}

pub(crate) fn build(engine: &mut TextEngine, output: &Output, slide: &Slide) -> Frame {
    let (w, h) = (output.width as f32, output.height as f32);
    let safe = safe_rect(output);
    let background = background_rect(output);
    let mut layout = SlideLayout { width: output.width, height: output.height, safe, background, text: None, image: None };
    let mut items = Vec::new();
    let mut texts = Vec::new();

    if let Content::Calibration { label, active } = &slide.content {
        let pattern = calibration::pattern(output.width, output.height, &output.safe_area, label, *active);
        items.extend(pattern.quads.into_iter().map(|q| Item::Solid { rect: q.rect, color: q.color }));
        for l in pattern.labels {
            texts.push(engine.label(&l.text, l.size, l.x, l.y, l.color));
        }
        layout.background = Rect { x: 0.0, y: 0.0, width: w, height: h };
        return Frame { layout, items, texts };
    }

    match &slide.background {
        Background::Color(color) => items.push(Item::Solid { rect: background, color: *color }),
        Background::Image(image) => items.push(Item::Image {
            rect: background,
            uv: cover_uv(background, image.aspect()),
            image: image.clone(),
        }),
    }

    match &slide.content {
        Content::Blank | Content::Calibration { .. } => {}
        Content::Text(text) => {
            let fitted = engine.fit(text, &output.text_style, safe, h);
            layout.text = Some(fitted.layout);
            texts.extend(fitted.placed);
        }
        Content::Image(image) => {
            let rect = output.safe_area.fit(w, h, image.aspect());
            layout.image = Some(rect);
            items.push(Item::Image { rect, uv: [0.0, 0.0, 1.0, 1.0], image: image.clone() });
        }
    }

    if output.show_safe_area {
        let t = 2.0 * calibration::line_width(w, h);
        items.extend(outline(safe, t, SAFE_AREA_OVERLAY).into_iter().map(|q| Item::Solid { rect: q.rect, color: q.color }));
    }

    Frame { layout, items, texts }
}

/// Lay out `slide` on `output` without drawing it: what the editor uses to
/// flag overflowing slides before the service (TS-7).
pub fn layout_slide(engine: &mut TextEngine, output: &Output, slide: &Slide) -> SlideLayout {
    build(engine, output, slide).layout
}
