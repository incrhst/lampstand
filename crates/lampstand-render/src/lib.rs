//! Render engine: one GPU-accelerated (wgpu) renderer per output, applying
//! that output's layout, theme and safe area.
//!
//! PRD: LV-3, LV-4, TS-1 to TS-10, OB-1 to OB-3.
//!
//! # Pieces
//!
//! - [`scene`]: what an output shows. An [`Output`] holds per-output settings
//!   (size, safe area, background bleed or fit, text style); a [`Slide`] is a
//!   background plus text, a pre-rendered image, or the calibration grid.
//! - [`layout`] and [`layout_slide`]: where things go, on the CPU. The editor
//!   can call [`layout_slide`] to flag overflowing slides without a GPU.
//! - [`Renderer`]: draws a prepared slide into any `wgpu` texture view.
//! - [`HeadlessRenderer`]: renders to an off-screen texture and reads it back,
//!   for CI tests and previews.
//! - [`window`]: display enumeration and borderless fullscreen output windows
//!   with the cursor hidden.
//!
//! # Text stack: cosmic-text + glyphon
//!
//! Text is shaped and laid out with [cosmic-text] and drawn with [glyphon],
//! its wgpu renderer. Reasons:
//!
//! - It does real shaping (HarfBuzz-compatible, via harfrust), bidi and font
//!   fallback, which scripture in Arabic, Hebrew or Indic scripts needs.
//! - Layout and drawing use the same shaped buffer, so the size and line
//!   breaks reported to the operator are exactly what the projector shows,
//!   and shrink-to-fit can measure without touching the GPU.
//! - glyphon caches glyphs in a texture atlas: after the first frame a slide
//!   change is one small upload and one draw call, well inside the 50 ms
//!   click-to-frame budget, and it needs nothing beyond WebGL2-level limits,
//!   so it runs on integrated graphics.
//! - Licences: cosmic-text and glyphon are MIT/Apache-2.0 (glyphon also
//!   Zlib), wgpu MIT/Apache-2.0, winit Apache-2.0: all GPL-3.0 compatible.
//!
//! Alternatives considered: `wgpu_text`/`glyph_brush` (no complex-script
//! shaping), Parley + Vello (strong, but Vello's compute shaders need more
//! than old integrated GPUs offer; worth revisiting for the slide engine).
//!
//! The bundled font is Noto Sans (SIL OFL 1.1, `assets/fonts`). It is loaded
//! as data, not linked as code, and the OFL permits bundling it with any
//! software. Only the bundled font is loaded by default, so layout is the
//! same on every machine and in CI; themes add fonts with
//! [`TextEngine::add_font`].
//!
//! [cosmic-text]: https://github.com/pop-os/cosmic-text
//! [glyphon]: https://github.com/grovesNL/glyphon

pub mod calibration;
mod frame;
mod gpu;
pub mod headless;
pub mod layout;
pub mod scene;
mod text;
pub mod window;

pub use frame::{layout_slide, SAFE_AREA_OVERLAY};
pub use gpu::{Gpu, RenderError, Renderer};
pub use headless::HeadlessRenderer;
pub use layout::{SlideLayout, TextLayout};
pub use lampstand_core::safe_area::{Rect, SafeArea};
pub use scene::{Background, BackgroundMode, Color, Content, Image, Output, Slide, TextSlide, TextStyle};
pub use text::{TextEngine, BUNDLED_FAMILY, BUNDLED_FONT};
pub use wgpu;
pub use winit;
