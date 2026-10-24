//! What is on air: the contract between the live engine
//! (`lampstand-live`), which decides it, and the compositor
//! (`lampstand-layout`), which draws it for each output.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::brand::{Brand, BugCorner, BugSize};
use crate::deck::DeckId;
use crate::geom::Color;
use crate::id::{BlobHash, Timestamp};
use crate::scene::Scene;
use crate::song::SongId;
use crate::theme::{Background, ThemeId};

/// One slide of text, already split: songs by the theme's line count,
/// scripture by what fits the primary output. Every output shows the same
/// `SlideText`, each fitting it to its own size.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlideText {
    /// Section label ("V1") or reference ("John 3:16–17"), for the operator
    /// and stage display. Not projected.
    pub label: Option<String>,
    pub lines: Vec<String>,
    /// A smaller line at the foot: song copyright and CCLI number, or the
    /// scripture reference and translation.
    pub footer: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Content {
    Lyrics {
        song: SongId,
        slide: SlideText,
    },
    Scripture {
        slide: SlideText,
    },
    Text {
        slide: SlideText,
    },
    /// A deck slide at a build step. `scene` is the slide in slide points,
    /// shared with the parsed deck. Step 0 is before any click.
    DeckSlide {
        deck: DeckId,
        index: usize,
        step: usize,
        scene: Arc<Scene>,
        notes: String,
    },
    Image {
        blob: BlobHash,
    },
    Video {
        blob: BlobHash,
        looped: bool,
    },
    Countdown {
        target: Timestamp,
        label: String,
    },
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Overlay {
    #[default]
    None,
    Black,
    Logo,
}

/// What an audience screen shows while nothing is on air, already resolved
/// for that screen (see `output::idle_plan`). Black and Logo cover it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Idle {
    Image {
        blob: BlobHash,
    },
    /// Loops, muted.
    Video {
        blob: BlobHash,
    },
    /// A deck slide in slide points, with its builds played.
    Deck {
        scene: Arc<Scene>,
    },
    /// The default: a logo centred on a plain background with a name quietly
    /// beneath it. The church's own logo and name when it has set a logo,
    /// else the Lampstand lamp.
    Mark(Mark),
}

/// The Lampstand lamp's dark background, from the app icon.
pub const LAMPSTAND_BACKGROUND: Color = Color::rgb(0x10, 0x14, 0x1f);
/// The Lampstand lamp's stand and wordmark colour, from the app icon.
pub const LAMPSTAND_TEXT: Color = Color::rgb(0xf4, 0xf1, 0xea);

/// What the default idle screen draws.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mark {
    /// The church's logo, contained in the middle of the screen. `None`
    /// draws the Lampstand lamp instead.
    pub logo: Option<BlobHash>,
    /// Drawn under the logo; empty draws no name.
    pub name: String,
    pub background: Color,
    pub text: Color,
}

impl Idle {
    /// The default idle screen: the church's logo and name when `brand`
    /// has a logo, else Lampstand's lamp and name. A brand with a name but
    /// no logo still gets the Lampstand lamp, so the screen is never bare.
    pub fn mark(brand: Option<&Brand>) -> Idle {
        match brand.filter(|b| b.logo.is_some()) {
            Some(brand) => Idle::Mark(Mark {
                logo: brand.logo,
                name: brand.name.trim().to_string(),
                background: brand.primary,
                text: brand.text_color(),
            }),
            None => Idle::Mark(Mark {
                logo: None,
                name: "Lampstand".into(),
                background: LAMPSTAND_BACKGROUND,
                text: LAMPSTAND_TEXT,
            }),
        }
    }
}

/// The church's logo screen, for the Logo overlay.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Logo {
    /// `None` until the church has chosen one: a ring is drawn instead.
    pub image: Option<BlobHash>,
    pub background: Color,
    /// The colour of the ring.
    pub accent: Color,
    /// The logo bug to draw in a corner over what is on air, for outputs
    /// that show it (audience and stream). `None` when it is off, there is
    /// no logo, or the output does not show it.
    #[serde(default)]
    pub bug: Option<Bug>,
}

/// The logo drawn small in a corner of the screen. The compositor shows it
/// unless Black or Logo covers the screen, or the idle screen is the logo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bug {
    pub image: BlobHash,
    /// The picture's width over its height, so it is drawn tight to the
    /// corner.
    pub aspect: f32,
    pub corner: BugCorner,
    pub size: BugSize,
    /// 0 to 1.
    pub opacity: f32,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Program {
    pub background: Option<Background>,
    pub content: Option<Content>,
    /// Covers everything when set. Toggles that persist across slides.
    pub overlay: Overlay,
    /// The slide after this one, for the stage display.
    pub next: Option<Content>,
    pub theme: Option<ThemeId>,
    /// Set by the app for an audience output when nothing is on air; the
    /// engine never sets it.
    #[serde(default)]
    pub idle: Option<Idle>,
    /// Set by the app from the church's brand, for the Logo overlay.
    #[serde(default)]
    pub logo: Option<Logo>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brand(name: &str, logo: Option<BlobHash>) -> Brand {
        Brand {
            name: name.into(),
            logo,
            primary: Color::rgb(250, 250, 250),
            text: None,
            accent: Color::rgb(200, 0, 0),
            font_family: String::new(),
            bug: Default::default(),
        }
    }

    fn mark(idle: Idle) -> Mark {
        let Idle::Mark(m) = idle else {
            panic!("expected the default idle screen")
        };
        m
    }

    #[test]
    fn without_a_brand_the_default_idle_screen_is_the_lampstand_lamp() {
        let m = mark(Idle::mark(None));
        assert_eq!((m.logo, m.name.as_str()), (None, "Lampstand"));
        assert_eq!(m.background, LAMPSTAND_BACKGROUND);
    }

    #[test]
    fn a_brand_without_a_logo_still_gets_the_lamp() {
        let m = mark(Idle::mark(Some(&brand("Grace Chapel", None))));
        assert_eq!((m.logo, m.name.as_str()), (None, "Lampstand"));
    }

    #[test]
    fn a_church_logo_brings_its_name_and_colours() {
        let logo = BlobHash::of(b"logo");
        let m = mark(Idle::mark(Some(&brand(" Grace Chapel ", Some(logo)))));
        assert_eq!(m.logo, Some(logo));
        assert_eq!(m.name, "Grace Chapel");
        assert_eq!(m.background, Color::rgb(250, 250, 250));
        assert_eq!(m.text, Color::BLACK, "dark text on a light background");
    }
}
