//! Composing what is on air into one output's scene (T23).

use lampstand_core::output::OutputConfig;
use lampstand_core::program::Program;
use lampstand_core::scene::Scene;
use lampstand_core::theme::Theme;

use crate::Fonts;

/// Output pixel size of an output, for composition.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OutputSize {
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Composed {
    pub scene: Scene,
    pub warnings: Vec<FitWarning>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FitWarning {
    BelowMinimum { size: f32 },
}

/// Draws what is on air for one output:
/// - Audience: background (bleeding or fitted per `BackgroundMode`), text
///   in the theme's text area inside the safe area, deck slides fitted to
///   the safe area keeping aspect ratio, overlays on top.
/// - Stage: current slide (deck thumbnail or text), next slide, notes,
///   clock and countdown, in a fixed high-contrast layout.
/// - Stream: transparent background, text as a lower third, deck slides
///   full frame.
///
/// Theme sizes scale by `size.height / REFERENCE_HEIGHT`.
pub fn compose(
    program: &Program,
    output: &OutputConfig,
    size: OutputSize,
    theme: &Theme,
    fonts: &mut Fonts,
) -> Composed {
    let _ = (program, output, size, theme, fonts);
    todo!("T23")
}
