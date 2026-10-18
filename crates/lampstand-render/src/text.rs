//! Text shaping and shrink-to-fit, on cosmic-text (see the crate docs for
//! why). The same shaped buffers are measured here and drawn by glyphon, so
//! what the layout reports is exactly what the projector shows.

use glyphon::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Wrap};
use lampstand_core::safe_area::Rect;

use crate::layout::{text_column, LineBox, TextLayout};
use crate::scene::{Color, TextSlide, TextStyle};

/// Noto Sans Regular (SIL Open Font License 1.1, see
/// `assets/fonts/NotoSans-LICENSE`). Bundled so layout is identical on every
/// machine and in CI; covers Latin, Greek and Cyrillic.
pub const BUNDLED_FONT: &[u8] = include_bytes!("../assets/fonts/NotoSans-Regular.ttf");
pub const BUNDLED_FAMILY: &str = "Noto Sans";

/// Fonts available to the renderer. Starts with only the bundled font, so
/// results never depend on what the host has installed.
pub struct TextEngine {
    pub(crate) fonts: FontSystem,
    family: String,
}

impl Default for TextEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Text placed on the output, ready for glyphon.
pub(crate) struct PlacedText {
    pub buffer: Buffer,
    pub left: f32,
    pub top: f32,
    pub clip: Rect,
    pub color: Color,
}

pub(crate) struct FittedText {
    pub layout: TextLayout,
    pub placed: Vec<PlacedText>,
}

impl TextEngine {
    pub fn new() -> Self {
        let mut db = glyphon::fontdb::Database::new();
        db.load_font_data(BUNDLED_FONT.to_vec());
        db.set_sans_serif_family(BUNDLED_FAMILY);
        TextEngine {
            fonts: FontSystem::new_with_locale_and_db("en-US".into(), db),
            family: BUNDLED_FAMILY.into(),
        }
    }

    /// Add a font (TTF/OTF bytes), e.g. one named by a theme.
    pub fn add_font(&mut self, data: Vec<u8>) {
        self.fonts.db_mut().load_font_data(data);
    }

    /// Family used for all text until themes choose fonts (LV-4).
    pub fn set_family(&mut self, family: impl Into<String>) {
        self.family = family.into();
    }

    fn buffer(&mut self, text: &str, size: f32, line_height: f32, width: Option<f32>) -> Buffer {
        let mut buffer = Buffer::new(&mut self.fonts, Metrics::new(size, size * line_height));
        buffer.set_wrap(Wrap::Word);
        buffer.set_size(width, None);
        let attrs = Attrs::new().family(Family::Name(&self.family));
        buffer.set_text(text, &attrs, Shaping::Advanced, Some(glyphon::cosmic_text::Align::Center));
        buffer.shape_until_scroll(&mut self.fonts, false);
        buffer
    }

    fn resize(&mut self, buffer: &mut Buffer, size: f32, line_height: f32) {
        buffer.set_metrics(Metrics::new(size, size * line_height));
        buffer.shape_until_scroll(&mut self.fonts, false);
    }

    /// Lay out `slide` in the safe area, shrinking the body from the style's
    /// maximum size towards its minimum until it fits (TS-7).
    pub(crate) fn fit(
        &mut self,
        slide: &TextSlide,
        style: &TextStyle,
        safe: Rect,
        output_height: f32,
    ) -> FittedText {
        let column = text_column(safe, style);
        let mut placed = Vec::new();

        // The footer keeps its size and sits at the bottom of the column.
        let mut footer_lines = Vec::new();
        let mut body_height = column.height;
        if let Some(text) = slide.footer.as_deref().filter(|t| !t.trim().is_empty()) {
            let size = style.footer_size * output_height;
            let buffer = self.buffer(text, size, style.line_height, Some(column.width));
            let (h, _) = measure(&buffer);
            let top = column.y + column.height - h;
            footer_lines = line_boxes(&buffer, column.x, top);
            body_height = (column.height - h - size * 0.5).max(0.0);
            placed.push(PlacedText { buffer, left: column.x, top, clip: column, color: style.color });
        }

        let max = style.max_size * output_height;
        let min = (style.min_size * output_height).min(max);
        let mut buffer = self.buffer(&slide.body, max, style.line_height, Some(column.width));
        let fits = |b: &Buffer| {
            let (h, w) = measure(b);
            h <= body_height + 0.5 && w <= column.width + 0.5
        };
        // Lyrics read best with each authored line on one line, so first
        // shrink to avoid wrapping; only if that is impossible above the
        // minimum (long scripture paragraphs) allow wrapping.
        let unwrapped = |b: &Buffer| fits(b) && b.layout_runs().count() <= b.lines.len();

        let mut size = None;
        for ok in [&unwrapped as &dyn Fn(&Buffer) -> bool, &fits] {
            size = self.largest(&mut buffer, min, max, style.line_height, ok);
            if size.is_some() {
                break;
            }
        }
        let overflow = size.is_none();
        let size = size.unwrap_or(min);
        self.resize(&mut buffer, size, style.line_height);

        let (h, _) = measure(&buffer);
        let top = if overflow { column.y } else { column.y + (body_height - h) / 2.0 };
        let lines = line_boxes(&buffer, column.x, top);
        let clip = Rect { height: body_height, ..column };
        placed.push(PlacedText { buffer, left: column.x, top, clip, color: style.color });

        FittedText {
            layout: TextLayout { column, font_size: size, overflow, lines, footer: footer_lines },
            placed,
        }
    }

    /// Largest size in `min..=max`, to a quarter pixel, at which `ok`
    /// holds, or `None` if it fails even at `min`.
    fn largest(
        &mut self,
        buffer: &mut Buffer,
        min: f32,
        max: f32,
        line_height: f32,
        ok: &dyn Fn(&Buffer) -> bool,
    ) -> Option<f32> {
        self.resize(buffer, max, line_height);
        if ok(buffer) {
            return Some(max);
        }
        self.resize(buffer, min, line_height);
        if !ok(buffer) {
            return None;
        }
        let (mut lo, mut hi) = (min, max);
        while hi - lo > 0.25 {
            let mid = (lo + hi) / 2.0;
            self.resize(buffer, mid, line_height);
            if ok(buffer) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        Some(((lo * 4.0).floor() / 4.0).max(min))
    }

    /// A single unwrapped label of `size` pixels centred on (`cx`, `cy`).
    pub(crate) fn label(&mut self, text: &str, size: f32, cx: f32, cy: f32, color: Color) -> PlacedText {
        let mut buffer = self.buffer(text, size, 1.2, None);
        let (h, w) = measure(&buffer);
        // Re-shape at its own width so multi-line labels centre each line.
        buffer.set_size(Some(w.ceil()), None);
        buffer.shape_until_scroll(&mut self.fonts, false);
        let (left, top) = (cx - w / 2.0, cy - h / 2.0);
        let clip = Rect { x: left - 1.0, y: top - 1.0, width: w + 2.0, height: h + 2.0 };
        PlacedText { buffer, left, top, clip, color }
    }
}

/// (height, widest line) of a shaped buffer.
fn measure(buffer: &Buffer) -> (f32, f32) {
    buffer.layout_runs().fold((0.0f32, 0.0f32), |(h, w), run| {
        (h.max(run.line_top + run.line_height), w.max(run.line_w))
    })
}

fn line_boxes(buffer: &Buffer, left: f32, top: f32) -> Vec<LineBox> {
    buffer
        .layout_runs()
        .filter(|run| !run.glyphs.is_empty())
        .map(|run| {
            let x0 = run.glyphs.iter().map(|g| g.x).fold(f32::MAX, f32::min);
            let x1 = run.glyphs.iter().map(|g| g.x + g.w).fold(f32::MIN, f32::max);
            let start = run.glyphs.iter().map(|g| g.start).min().unwrap_or(0);
            let end = run.glyphs.iter().map(|g| g.end).max().unwrap_or(0);
            LineBox {
                rect: Rect {
                    x: left + x0,
                    y: top + run.line_top,
                    width: x1 - x0,
                    height: run.line_height,
                },
                text: run.text[start..end].trim().to_string(),
            }
        })
        .collect()
}
