//! Where things go on an output, in physical pixels. Pure CPU, so it can be
//! tested without a GPU or display.

use std::fmt;

use lampstand_core::safe_area::Rect;

use crate::scene::{BackgroundMode, Output, TextStyle};

/// The safe area of `output` in pixels (TS-2).
pub fn safe_rect(output: &Output) -> Rect {
    output.safe_area.rect(output.width as f32, output.height as f32)
}

/// The area the background covers: the whole screen when it bleeds, the
/// safe area when it fits (TS-3).
pub fn background_rect(output: &Output) -> Rect {
    match output.background_mode {
        BackgroundMode::Bleed => full_rect(output),
        BackgroundMode::Fit => safe_rect(output),
    }
}

pub fn full_rect(output: &Output) -> Rect {
    Rect { x: 0.0, y: 0.0, width: output.width as f32, height: output.height as f32 }
}

/// The column text is laid out in: the safe area, narrowed and centred on
/// very wide outputs so lines stay short enough to read (TS-9).
pub fn text_column(safe: Rect, style: &TextStyle) -> Rect {
    let max_width = safe.height * style.max_column_aspect;
    if safe.width <= max_width {
        return safe;
    }
    Rect { x: safe.x + (safe.width - max_width) / 2.0, width: max_width, ..safe }
}

/// Texture coordinates `[u0, v0, u1, v1]` that crop an image of `aspect`
/// (width / height) to cover `rect` without stretching.
pub fn cover_uv(rect: Rect, aspect: f32) -> [f32; 4] {
    let rect_aspect = rect.width / rect.height;
    if aspect > rect_aspect {
        let visible = rect_aspect / aspect;
        let u0 = (1.0 - visible) / 2.0;
        [u0, 0.0, u0 + visible, 1.0]
    } else {
        let visible = aspect / rect_aspect;
        let v0 = (1.0 - visible) / 2.0;
        [0.0, v0, 1.0, v0 + visible]
    }
}

/// One laid-out line of text, in output pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct LineBox {
    pub rect: Rect,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextLayout {
    pub column: Rect,
    /// Body font size in pixels after shrinking to fit.
    pub font_size: f32,
    /// True when the body does not fit even at the minimum size (TS-7). The
    /// renderer clips it to the safe area; the editor should flag the slide.
    pub overflow: bool,
    pub lines: Vec<LineBox>,
    pub footer: Vec<LineBox>,
}

/// Everything the renderer decided about a slide on one output.
#[derive(Debug, Clone, PartialEq)]
pub struct SlideLayout {
    pub width: u32,
    pub height: u32,
    pub safe: Rect,
    pub background: Rect,
    pub text: Option<TextLayout>,
    /// Where a pre-rendered slide image is drawn (TS-4).
    pub image: Option<Rect>,
}

impl SlideLayout {
    /// True when text had to be clipped; show a warning before the service.
    pub fn overflow(&self) -> bool {
        self.text.as_ref().is_some_and(|t| t.overflow)
    }
}

struct R(Rect);

impl fmt::Display for R {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let r = self.0;
        write!(f, "{:.0},{:.0} {:.0}x{:.0}", r.x, r.y, r.width, r.height)
    }
}

/// A stable, readable form used for snapshot tests.
impl fmt::Display for SlideLayout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "output {}x{}", self.width, self.height)?;
        writeln!(f, "safe {}", R(self.safe))?;
        writeln!(f, "background {}", R(self.background))?;
        if let Some(image) = self.image {
            writeln!(f, "image {}", R(image))?;
        }
        if let Some(text) = &self.text {
            writeln!(f, "column {}", R(text.column))?;
            writeln!(f, "size {:.2} overflow {}", text.font_size, text.overflow)?;
            for line in &text.lines {
                writeln!(f, "  {} {:?}", R(line.rect), line.text)?;
            }
            for line in &text.footer {
                writeln!(f, "  footer {} {:?}", R(line.rect), line.text)?;
            }
        }
        Ok(())
    }
}

/// True when `inner` lies within `outer`, allowing half a pixel of rounding.
pub fn contains(outer: Rect, inner: Rect) -> bool {
    const EPS: f32 = 0.5;
    inner.x >= outer.x - EPS
        && inner.y >= outer.y - EPS
        && inner.x + inner.width <= outer.x + outer.width + EPS
        && inner.y + inner.height <= outer.y + outer.height + EPS
}
