//! Product names for the connected displays.
//!
//! Tauri names a Mac display "Monitor #<model number>", which tells an
//! operator nothing. The operating system knows what the display calls
//! itself ("LG UltraFine", "EPSON EB-L200F"), so the app asks it and swaps
//! the name in. A display the OS cannot name keeps the generic one.

use lampstand_render::output::DisplayInfo;

/// A display as the operating system names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Native {
    pub name: String,
    /// Physical pixels, the unit `DisplayInfo` uses.
    pub width: u32,
    pub height: u32,
}

/// The displays the OS names. Must run on the main thread; elsewhere, and
/// on platforms without a lookup yet, it finds none.
#[cfg(target_os = "macos")]
pub fn native_displays() -> Vec<Native> {
    use objc2_app_kit::NSScreen;
    use objc2_foundation::MainThreadMarker;

    let Some(mtm) = MainThreadMarker::new() else {
        return Vec::new();
    };
    let screens = NSScreen::screens(mtm);
    (0..screens.count())
        .map(|i| {
            // SAFETY: `i` is below `count`.
            let screen = unsafe { screens.objectAtIndex(i) };
            let scale = screen.backingScaleFactor();
            let frame = screen.frame();
            // SAFETY: `localizedName` is a plain getter, available on
            // macOS 10.15 and later.
            let name = unsafe { screen.localizedName() }.to_string();
            Native {
                name,
                width: (frame.size.width * scale).round() as u32,
                height: (frame.size.height * scale).round() as u32,
            }
        })
        .collect()
}

#[cfg(not(target_os = "macos"))]
pub fn native_displays() -> Vec<Native> {
    Vec::new()
}

/// Gives each display the OS name of the one native display with its pixel
/// size. Two displays of one size cannot be told apart from here, so they
/// keep the generic name rather than risk swapping them.
pub fn apply(displays: &mut [DisplayInfo], native: &[Native]) {
    for d in displays.iter_mut() {
        let mut same = native
            .iter()
            .filter(|n| n.width == d.width && n.height == d.height && !n.name.is_empty());
        if let (Some(n), None) = (same.next(), same.next()) {
            d.name = n.name.clone();
        }
    }
}

/// Drops the display the operator window is on, given as that monitor's
/// position and size. A full-screen output there would cover the controls
/// and the cursor would be hidden, so it is never offered and an output
/// saved for it stays closed. `None` (no operator window, or no monitor
/// for it) drops nothing.
pub fn without_operator(displays: &mut Vec<DisplayInfo>, operator: Option<DisplayInfo>) {
    let Some(op) = operator else {
        return;
    };
    displays.retain(|d| (d.x, d.y, d.width, d.height) != (op.x, op.y, op.width, op.height));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn display(name: &str, width: u32, height: u32) -> DisplayInfo {
        DisplayInfo {
            name: name.into(),
            width,
            height,
            x: 0,
            y: 0,
        }
    }

    fn native(name: &str, width: u32, height: u32) -> Native {
        Native {
            name: name.into(),
            width,
            height,
        }
    }

    #[test]
    fn a_display_takes_the_name_of_the_one_native_display_its_size() {
        let mut displays = vec![
            display("Monitor #1", 2880, 1800),
            display("Monitor #2", 1920, 1080),
        ];
        let found = [
            native("Built-in Retina Display", 2880, 1800),
            native("EPSON EB-L200F", 1920, 1080),
        ];
        apply(&mut displays, &found);
        assert_eq!(displays[0].name, "Built-in Retina Display");
        assert_eq!(displays[1].name, "EPSON EB-L200F");
    }

    #[test]
    fn two_displays_of_one_size_keep_the_generic_name() {
        let mut displays = vec![
            display("Monitor #1", 1920, 1080),
            display("Monitor #2", 1920, 1080),
        ];
        let found = [
            native("Projector A", 1920, 1080),
            native("Projector B", 1920, 1080),
        ];
        apply(&mut displays, &found);
        assert_eq!(displays[0].name, "Monitor #1");
        assert_eq!(displays[1].name, "Monitor #2");
    }

    #[test]
    fn a_display_the_os_cannot_name_keeps_the_generic_name() {
        let mut displays = vec![display("Monitor #1", 1920, 1080)];
        apply(
            &mut displays,
            &[native("", 1920, 1080), native("Other", 1280, 720)],
        );
        assert_eq!(displays[0].name, "Monitor #1");
    }

    fn at(x: i32, width: u32, height: u32) -> DisplayInfo {
        DisplayInfo {
            x,
            ..display("Monitor", width, height)
        }
    }

    #[test]
    fn the_operator_display_is_never_offered() {
        let mut displays = vec![at(0, 2880, 1800), at(2880, 1920, 1080)];
        without_operator(&mut displays, Some(at(0, 2880, 1800)));
        assert_eq!(displays, vec![at(2880, 1920, 1080)]);
    }

    #[test]
    fn a_lone_display_that_is_the_operators_leaves_none() {
        let mut displays = vec![at(0, 2880, 1800)];
        without_operator(&mut displays, Some(at(0, 2880, 1800)));
        assert!(displays.is_empty());
    }

    #[test]
    fn without_a_known_operator_display_every_display_stays() {
        let mut displays = vec![at(0, 2880, 1800), at(2880, 1920, 1080)];
        without_operator(&mut displays, None);
        assert_eq!(displays.len(), 2);
    }
}
