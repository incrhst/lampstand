//! `LibraryRequest::ThemePreview`: a theme drawn with sample text by the
//! same layout and renderer the screens use, so what the Themes page shows
//! is what the congregation will see.

use lampstand_api::ThemeSample;
use lampstand_core::brand::Brand;
use lampstand_core::id::Timestamp;
use lampstand_core::output::{BackgroundMode, OutputConfig, OutputId, OutputRole, OutputTarget};
use lampstand_core::program::{Content, Idle, Program, SlideText};
use lampstand_core::safe_area::SafeArea;
use lampstand_core::song::SongId;
use lampstand_core::theme::Theme;
use lampstand_layout::{compose_at, Fonts, OutputSize};

use crate::decks;

/// The sample as a slide, split with the theme's lines per slide the way a
/// song would be.
fn content(theme: &Theme, sample: ThemeSample) -> Content {
    let lines = |all: &[&str]| -> Vec<String> {
        all.iter()
            .take(usize::from(theme.max_lines).max(1))
            .map(|l| (*l).to_string())
            .collect()
    };
    match sample {
        ThemeSample::Lyrics => Content::Lyrics {
            song: SongId::generate(),
            slide: SlideText {
                label: Some("V1".into()),
                lines: lines(&[
                    "Amazing grace! How sweet the sound",
                    "That saved a wretch like me!",
                    "I once was lost, but now am found",
                    "Was blind, but now I see.",
                ]),
                footer: theme
                    .show_copyright
                    .then(|| "John Newton · Public domain".into()),
            },
        },
        ThemeSample::Verse => Content::Scripture {
            slide: SlideText {
                label: Some("John 3:16".into()),
                lines: vec![
                    "For God so loved the world, that he gave his only begotten Son, \
                     that whosoever believeth in him should not perish, but have \
                     everlasting life."
                        .into(),
                ],
                footer: Some("John 3:16 KJV".into()),
            },
        },
        ThemeSample::Text => Content::Text {
            slide: SlideText {
                label: None,
                lines: vec![
                    "Welcome".into(),
                    "Coffee and tea after the service in the hall".into(),
                ],
                footer: None,
            },
        },
    }
}

/// A PNG `data:` URL, `thumb_width` pixels wide, of `theme` showing
/// `sample` on a screen `width` by `height` pixels in shape.
pub fn preview(
    theme: &Theme,
    sample: ThemeSample,
    (width, height): (u32, u32),
    thumb_width: u32,
    fonts: &std::sync::Mutex<Option<Fonts>>,
    renderer: &lampstand_render::Renderer,
) -> Result<String, String> {
    let (width, height) = (width.clamp(160, 7680), height.clamp(90, 4320));
    let output = OutputConfig {
        id: OutputId::generate(),
        name: "Preview".into(),
        role: OutputRole::Audience { primary: true },
        target: OutputTarget::Window { width, height },
        safe_area: SafeArea::default(),
        background: BackgroundMode::Bleed,
        theme: None,
        idle: None,
        enabled: true,
    };
    let program = Program {
        content: Some(content(theme, sample)),
        ..Program::default()
    };
    let composed = {
        let mut fonts = fonts.lock().unwrap_or_else(|e| e.into_inner());
        compose_at(
            &program,
            &output,
            OutputSize {
                width: width as f32,
                height: height as f32,
            },
            theme,
            fonts.get_or_insert_with(Fonts::load),
            Timestamp(0),
            0,
        )
    };
    let (w, h) = decks::thumbnail_size(&composed.scene, thumb_width);
    let image = renderer
        .render_offscreen(&composed.scene, w, h)
        .map_err(|e| format!("{e:?}"))?;
    decks::png_data_url(&image)
}

/// The brand the default idle screen is drawn from: `brand` without its
/// logo when that picture has not reached this device, so the screen shows
/// the Lampstand lamp rather than a hole.
pub fn brand_for_idle(library: &lampstand_library::Library, brand: Option<Brand>) -> Option<Brand> {
    brand.map(|mut b| {
        if b.logo
            .is_some_and(|l| library.repo().blobs().path(&l).is_none())
        {
            b.logo = None;
        }
        b
    })
}

/// A PNG `data:` URL, `thumb_width` pixels wide and 16:9, of the default
/// idle screen for `brand`.
pub fn idle_preview(
    brand: Option<&Brand>,
    theme: &Theme,
    thumb_width: u32,
    fonts: &std::sync::Mutex<Option<Fonts>>,
    renderer: &lampstand_render::Renderer,
) -> Result<String, String> {
    let (width, height) = (1920, 1080);
    let output = OutputConfig {
        id: OutputId::generate(),
        name: "Preview".into(),
        role: OutputRole::Audience { primary: true },
        target: OutputTarget::Window { width, height },
        safe_area: SafeArea::default(),
        background: BackgroundMode::Bleed,
        theme: None,
        idle: None,
        enabled: true,
    };
    let program = Program {
        idle: Some(Idle::mark(brand)),
        ..Program::default()
    };
    let composed = {
        let mut fonts = fonts.lock().unwrap_or_else(|e| e.into_inner());
        compose_at(
            &program,
            &output,
            OutputSize {
                width: width as f32,
                height: height as f32,
            },
            theme,
            fonts.get_or_insert_with(Fonts::load),
            Timestamp(0),
            0,
        )
    };
    let (w, h) = decks::thumbnail_size(&composed.scene, thumb_width);
    let image = renderer
        .render_offscreen(&composed.scene, w, h)
        .map_err(|e| format!("{e:?}"))?;
    decks::png_data_url(&image)
}
