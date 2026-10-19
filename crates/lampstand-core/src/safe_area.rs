//! Per-output title-safe area: margins set by a volunteer in calibration
//! mode, inside which all text is laid out.
//!
//! PRD: TS-1 to TS-4.

use autosurgeon::{Hydrate, Reconcile};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Margins as fractions of the output's width and height (0.05 = 5%).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize, Reconcile, Hydrate)]
pub struct SafeArea {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl SafeArea {
    /// The rectangle text may use on an output of the given pixel size.
    pub fn rect(&self, width: f32, height: f32) -> Rect {
        let clamp = |m: f32| m.clamp(0.0, 0.45);
        let (t, r, b, l) = (
            clamp(self.top),
            clamp(self.right),
            clamp(self.bottom),
            clamp(self.left),
        );
        Rect {
            x: width * l,
            y: height * t,
            width: width * (1.0 - l - r),
            height: height * (1.0 - t - b),
        }
    }

    /// Largest rectangle of the given aspect ratio that fits the safe area,
    /// centred. Used to fit imported slide decks (TS-4).
    pub fn fit(&self, width: f32, height: f32, aspect: f32) -> Rect {
        let area = self.rect(width, height);
        let (w, h) = if area.width / area.height > aspect {
            (area.height * aspect, area.height)
        } else {
            (area.width, area.width / aspect)
        };
        Rect {
            x: area.x + (area.width - w) / 2.0,
            y: area.y + (area.height - h) / 2.0,
            width: w,
            height: h,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_applies_margins() {
        let safe = SafeArea {
            top: 0.1,
            right: 0.05,
            bottom: 0.25,
            left: 0.05,
        };
        assert_eq!(
            safe.rect(1000.0, 1000.0),
            Rect {
                x: 50.0,
                y: 100.0,
                width: 900.0,
                height: 650.0
            }
        );
    }

    #[test]
    fn margins_are_clamped() {
        let safe = SafeArea {
            top: 2.0,
            right: -1.0,
            bottom: 0.0,
            left: 0.0,
        };
        let r = safe.rect(100.0, 100.0);
        assert_eq!((r.y, r.height, r.width), (45.0, 55.0, 100.0));
    }

    #[test]
    fn fit_letterboxes_a_16_9_deck_on_a_4_3_screen() {
        let r = SafeArea::default().fit(1024.0, 768.0, 16.0 / 9.0);
        assert_eq!((r.x, r.width), (0.0, 1024.0));
        assert_eq!(r.height, 576.0);
        assert_eq!(r.y, 96.0);
    }
}
