//! A church's brand: its name, logo, colours and font. Lampstand turns it
//! into a starting theme (`lampstand_library::theme_from_brand`) and shows
//! the logo when the Logo overlay is on.
//!
//! The brand is one of the church's settings that follow it across its
//! devices (`crate::church`); the theme made from it is a library document.

use serde::{Deserialize, Serialize};

use crate::geom::{Color, Rect};
use crate::id::BlobHash;

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Brand {
    /// The church's name; the theme made from it is named after it.
    pub name: String,
    /// A picture in the blob store, shown by the Logo overlay.
    pub logo: Option<BlobHash>,
    /// The main colour: the background of the theme and of the logo screen.
    pub primary: Color,
    /// The colour of lyrics and scripture. `None` picks black or white,
    /// whichever reads better on `primary`.
    pub text: Option<Color>,
    /// A third colour, for the ring drawn when there is no logo yet.
    pub accent: Color,
    /// A font family name. Empty means the starter theme's.
    pub font_family: String,
    /// The logo drawn small in a corner of the audience screens and the
    /// stream, over everything but Black and the Logo screen.
    #[serde(default)]
    pub bug: LogoBug,
}

/// Which lower corner the logo bug sits in.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BugCorner {
    LowerLeft,
    #[default]
    LowerRight,
}

/// How big the logo bug is, as a fraction of the screen's height.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BugSize {
    #[default]
    Small,
    Medium,
    Large,
}

impl BugSize {
    /// The bug's height as a fraction of the screen's height.
    pub fn height_fraction(self) -> f32 {
        match self {
            BugSize::Small => 0.07,
            BugSize::Medium => 0.1,
            BugSize::Large => 0.14,
        }
    }
}

/// The logo bug setting: off until the church turns it on, then in the
/// lower right unless it chooses the lower left.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogoBug {
    pub enabled: bool,
    pub corner: BugCorner,
    pub size: BugSize,
    /// 10 to 100 per cent.
    pub opacity: u8,
}

impl Default for LogoBug {
    fn default() -> Self {
        LogoBug {
            enabled: false,
            corner: BugCorner::default(),
            size: BugSize::default(),
            opacity: 80,
        }
    }
}

impl LogoBug {
    /// `opacity` as 0 to 1, kept to the range where the bug stays visible.
    pub fn alpha(&self) -> f32 {
        f32::from(self.opacity.clamp(10, 100)) / 100.0
    }
}

/// The widest a bug may be, as a fraction of the safe area's width, so a
/// very wide logo cannot run across the screen.
const BUG_MAX_WIDTH: f32 = 0.25;

/// The gap between the bug and the safe area's edge, as a fraction of the
/// screen's height.
const BUG_MARGIN: f32 = 0.02;

/// Where the logo bug is drawn: the corner of `safe` (the safe area in
/// pixels) with a margin, `screen_height` pixels high in all, for a logo
/// `aspect` (width over height) wide. The rectangle has the logo's own
/// proportions, so the picture meets the corner without letterboxing, and
/// it never leaves `safe`.
pub fn bug_rect(
    safe: Rect,
    screen_height: f32,
    aspect: f32,
    corner: BugCorner,
    size: BugSize,
) -> Rect {
    let aspect = if aspect.is_finite() && aspect > 0.0 {
        aspect
    } else {
        1.0
    };
    let margin = screen_height * BUG_MARGIN;
    let room_w = (safe.width - 2.0 * margin).max(1.0);
    let room_h = (safe.height - 2.0 * margin).max(1.0);
    let mut height = screen_height * size.height_fraction();
    let mut width = height * aspect;
    let widest = (safe.width * BUG_MAX_WIDTH).min(room_w);
    if width > widest {
        width = widest;
        height = width / aspect;
    }
    if height > room_h {
        height = room_h;
        width = height * aspect;
    }
    let x = match corner {
        BugCorner::LowerLeft => safe.x + margin,
        BugCorner::LowerRight => safe.x + safe.width - margin - width,
    };
    Rect {
        x,
        y: safe.y + safe.height - margin - height,
        width,
        height,
    }
}

impl Brand {
    /// The colour lyrics and scripture are drawn in.
    pub fn text_color(&self) -> Color {
        self.text.unwrap_or_else(|| readable_on(self.primary))
    }
}

/// Black or white, whichever has more contrast against `background`.
pub fn readable_on(background: Color) -> Color {
    if luminance(background) > 0.4 {
        Color::BLACK
    } else {
        Color::WHITE
    }
}

/// Relative luminance (WCAG), 0 for black to 1 for white.
pub fn luminance(c: Color) -> f32 {
    fn channel(v: u8) -> f32 {
        let v = f32::from(v) / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b)
}

/// The contrast ratio between two colours, 1 to 21 (WCAG).
pub fn contrast(a: Color, b: Color) -> f32 {
    let (hi, lo) = {
        let (x, y) = (luminance(a), luminance(b));
        if x > y {
            (x, y)
        } else {
            (y, x)
        }
    };
    (hi + 0.05) / (lo + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_readable_on_dark_and_light() {
        assert_eq!(readable_on(Color::rgb(20, 30, 90)), Color::WHITE);
        assert_eq!(readable_on(Color::rgb(250, 240, 200)), Color::BLACK);
    }

    fn inside(inner: Rect, outer: Rect) -> bool {
        let eps = 0.01;
        inner.x >= outer.x - eps
            && inner.y >= outer.y - eps
            && inner.x + inner.width <= outer.x + outer.width + eps
            && inner.y + inner.height <= outer.y + outer.height + eps
    }

    #[test]
    fn the_bug_is_off_by_default_and_lower_right_when_turned_on() {
        let bug = LogoBug::default();
        assert!(!bug.enabled);
        assert_eq!(bug.corner, BugCorner::LowerRight);
        assert_eq!(bug.size, BugSize::Small);
        assert!((bug.alpha() - 0.8).abs() < 1e-6);
        // Too faint to see is raised; over full is capped.
        assert!((LogoBug { opacity: 0, ..bug }.alpha() - 0.1).abs() < 1e-6);
        assert!(
            (LogoBug {
                opacity: 250,
                ..bug
            }
            .alpha()
                - 1.0)
                .abs()
                < 1e-6
        );
    }

    #[test]
    fn a_brand_saved_before_the_bug_existed_still_loads() {
        let old = r#"{"name":"A","logo":null,"primary":{"r":0,"g":0,"b":0,"a":255},
            "text":null,"accent":{"r":0,"g":0,"b":0,"a":255},"font_family":""}"#;
        let brand: Brand = serde_json::from_str(old).unwrap();
        assert_eq!(brand.bug, LogoBug::default());
    }

    #[test]
    fn the_bug_sits_in_its_corner_inside_the_safe_area() {
        // 1920x1080 with a 5% crop all round.
        let safe = Rect {
            x: 96.0,
            y: 54.0,
            width: 1728.0,
            height: 972.0,
        };
        let right = bug_rect(safe, 1080.0, 1.0, BugCorner::LowerRight, BugSize::Medium);
        let left = bug_rect(safe, 1080.0, 1.0, BugCorner::LowerLeft, BugSize::Medium);
        assert!((right.height - 108.0).abs() < 0.01);
        assert!((right.width - 108.0).abs() < 0.01);
        // A margin of 2% of the height (21.6) from the safe edges.
        assert!((right.x + right.width - (96.0 + 1728.0 - 21.6)).abs() < 0.01);
        assert!((right.y + right.height - (54.0 + 972.0 - 21.6)).abs() < 0.01);
        assert!((left.x - (96.0 + 21.6)).abs() < 0.01);
        assert_eq!(left.y, right.y);
        assert!(inside(right, safe) && inside(left, safe));
    }

    #[test]
    fn the_bug_scales_with_the_size_and_the_screen() {
        let safe = |w: f32, h: f32| Rect {
            x: 0.0,
            y: 0.0,
            width: w,
            height: h,
        };
        let h = |size| {
            bug_rect(
                safe(1920.0, 1080.0),
                1080.0,
                1.0,
                BugCorner::LowerRight,
                size,
            )
            .height
        };
        assert!(h(BugSize::Small) < h(BugSize::Medium) && h(BugSize::Medium) < h(BugSize::Large));
        let small = bug_rect(
            safe(960.0, 540.0),
            540.0,
            1.0,
            BugCorner::LowerRight,
            BugSize::Small,
        );
        assert!((small.height - 540.0 * 0.07).abs() < 0.01);
    }

    #[test]
    fn the_bug_fits_every_shape_of_screen_and_logo() {
        let shapes = [
            (1920.0, 1080.0),
            (1280.0, 1024.0),
            (1024.0, 768.0),
            (3840.0, 1080.0),
            (1080.0, 1920.0),
        ];
        let cropped = [0.0_f32, 0.05, 0.1];
        let logos = [0.2, 1.0, 3.0, 8.0];
        for (w, h) in shapes {
            for crop in cropped {
                let safe = Rect {
                    x: w * crop,
                    y: h * crop,
                    width: w * (1.0 - 2.0 * crop),
                    height: h * (1.0 - 2.0 * crop),
                };
                for aspect in logos {
                    for size in [BugSize::Small, BugSize::Medium, BugSize::Large] {
                        for corner in [BugCorner::LowerLeft, BugCorner::LowerRight] {
                            let r = bug_rect(safe, h, aspect, corner, size);
                            assert!(
                                inside(r, safe),
                                "{w}x{h} crop {crop} aspect {aspect}: {r:?}"
                            );
                            assert!((r.width / r.height - aspect).abs() < 0.01);
                            assert!(r.width <= safe.width * BUG_MAX_WIDTH + 0.01);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_nonsense_logo_shape_falls_back_to_a_square() {
        let safe = Rect {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };
        let r = bug_rect(
            safe,
            1080.0,
            f32::NAN,
            BugCorner::LowerRight,
            BugSize::Small,
        );
        assert!((r.width - r.height).abs() < 0.01);
    }

    #[test]
    fn contrast_of_black_on_white_is_21() {
        assert!((contrast(Color::BLACK, Color::WHITE) - 21.0).abs() < 0.01);
    }
}
