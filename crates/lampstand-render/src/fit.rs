//! Shrink text to fit the safe area, down to a minimum size.
//!
//! PRD: TS-7.

/// The font size chosen for a slide.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fit {
    pub size: f32,
    /// The text does not fit even at the minimum size. The editor and
    /// preview flag such slides before the service.
    pub overflows: bool,
}

/// The largest whole-pixel size between `min` and `max` at which `fits`
/// holds. `fits` must be monotonic: true at a size means true at every
/// smaller size.
pub fn fit_size(min: f32, max: f32, mut fits: impl FnMut(f32) -> bool) -> Fit {
    let (mut lo, mut hi) = (min.floor() as u32, max.floor().max(min) as u32);
    if !fits(lo as f32) {
        return Fit {
            size: min,
            overflows: true,
        };
    }
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if fits(mid as f32) {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    Fit {
        size: lo as f32,
        overflows: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_largest_size_that_fits() {
        assert_eq!(
            fit_size(10.0, 100.0, |s| s <= 57.0),
            Fit {
                size: 57.0,
                overflows: false
            }
        );
    }

    #[test]
    fn uses_the_maximum_when_everything_fits() {
        assert_eq!(fit_size(10.0, 100.0, |_| true).size, 100.0);
    }

    #[test]
    fn flags_text_that_overflows_at_the_minimum() {
        assert_eq!(
            fit_size(10.0, 100.0, |_| false),
            Fit {
                size: 10.0,
                overflows: true
            }
        );
    }
}
