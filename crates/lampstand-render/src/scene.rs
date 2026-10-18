//! What an output shows: the per-output settings ([`Output`]) and the slide
//! currently live on it ([`Slide`]).

use std::sync::Arc;

use lampstand_core::safe_area::SafeArea;

use crate::calibration::Edge;

/// An sRGB colour with straight (not premultiplied) alpha.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    pub const WHITE: Color = Color::rgb(255, 255, 255);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Color { r, g, b, a: 255 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Color { r, g, b, a }
    }
}

/// An RGBA8 image in sRGB with straight alpha, row-major, no padding.
///
/// Slides rendered elsewhere (an imported deck, a DeckCraft or vello_cpu
/// frame) arrive as one of these and are scaled into the safe area (TS-4).
/// Share it behind an [`Arc`]: the renderer keeps the GPU copy for as long as
/// the same `Arc` stays on screen, so re-showing a slide does not re-upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    /// Panics if `rgba` is not `width * height * 4` bytes.
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Self {
        assert_eq!(rgba.len(), width as usize * height as usize * 4, "RGBA8 buffer size");
        Image { width, height, rgba }
    }

    pub fn aspect(&self) -> f32 {
        self.width as f32 / self.height as f32
    }
}

impl From<image::RgbaImage> for Image {
    fn from(img: image::RgbaImage) -> Self {
        Image { width: img.width(), height: img.height(), rgba: img.into_raw() }
    }
}

/// Whether the background fills the whole screen or stays inside the safe
/// area with black around it. Chosen per output (TS-3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BackgroundMode {
    #[default]
    Bleed,
    Fit,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Background {
    Color(Color),
    /// Scaled to cover its area (cropping, never stretching).
    Image(Arc<Image>),
}

impl Default for Background {
    fn default() -> Self {
        Background::Color(Color::BLACK)
    }
}

/// Text sizes are fractions of the output height, so a theme looks the same
/// at 720p and 4K and only the safe area and aspect ratio change the layout.
#[derive(Debug, Clone, PartialEq)]
pub struct TextStyle {
    pub color: Color,
    /// Size text starts at before shrinking to fit.
    pub max_size: f32,
    /// Smallest size text may shrink to; below this the slide overflows (TS-7).
    pub min_size: f32,
    /// Line height as a multiple of the font size.
    pub line_height: f32,
    /// Footer (scripture reference, song credit) size; it does not shrink.
    pub footer_size: f32,
    /// Widest the text column may be, as a multiple of its height. Keeps
    /// lines readable on ultra-wide LED walls instead of one long line (TS-9).
    pub max_column_aspect: f32,
}

impl Default for TextStyle {
    fn default() -> Self {
        TextStyle {
            color: Color::WHITE,
            max_size: 0.09,
            min_size: 0.04,
            line_height: 1.2,
            footer_size: 0.035,
            max_column_aspect: 2.4,
        }
    }
}

/// Per-output settings: one per projector, TV, stage display or stream (LV-3).
#[derive(Debug, Clone, PartialEq)]
pub struct Output {
    /// Size in physical pixels.
    pub width: u32,
    pub height: u32,
    pub safe_area: SafeArea,
    pub background_mode: BackgroundMode,
    pub text_style: TextStyle,
    /// Draw the safe area as an outline, for preview and live panes (TS-8).
    /// Leave off on the real projector.
    pub show_safe_area: bool,
}

impl Output {
    pub fn new(width: u32, height: u32) -> Self {
        Output {
            width,
            height,
            safe_area: SafeArea::default(),
            background_mode: BackgroundMode::default(),
            text_style: TextStyle::default(),
            show_safe_area: false,
        }
    }

    pub fn with_safe_area(mut self, safe_area: SafeArea) -> Self {
        self.safe_area = safe_area;
        self
    }
}

/// A lyric verse or scripture passage.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TextSlide {
    /// Lines separated by `\n`; long lines wrap at word boundaries.
    pub body: String,
    /// Smaller line at the bottom of the safe area, e.g. "John 3:16 (KJV)".
    pub footer: Option<String>,
}

impl TextSlide {
    pub fn new(body: impl Into<String>) -> Self {
        TextSlide { body: body.into(), footer: None }
    }

    pub fn with_footer(mut self, footer: impl Into<String>) -> Self {
        self.footer = Some(footer.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum Content {
    #[default]
    Blank,
    Text(TextSlide),
    /// A pre-rendered slide, fitted into the safe area keeping its aspect (TS-4).
    Image(Arc<Image>),
    /// Calibration grid for this output (TS-1); `label` names the output and
    /// `active` is the safe-area edge being adjusted, if any.
    Calibration { label: String, active: Option<Edge> },
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Slide {
    pub background: Background,
    pub content: Content,
}

impl Slide {
    pub fn text(slide: TextSlide) -> Self {
        Slide { content: Content::Text(slide), ..Default::default() }
    }

    pub fn image(image: Arc<Image>) -> Self {
        Slide { content: Content::Image(image), ..Default::default() }
    }

    pub fn calibration(label: impl Into<String>) -> Self {
        Slide { content: Content::Calibration { label: label.into(), active: None }, ..Default::default() }
    }

    pub fn with_background(mut self, background: Background) -> Self {
        self.background = background;
        self
    }
}
