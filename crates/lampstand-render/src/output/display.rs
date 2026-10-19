//! Finding each output's display again (TS-2, TS-10 later).
//!
//! The window system (winit in the demo and tests, Tauri in the app) lists
//! its monitors as `DisplayInfo`s; `assign_displays` decides which output
//! goes where. The rule is the one `DisplayMatch` documents: name, then
//! size, then position. A display that matches on neither name nor size is
//! never taken, so an unplugged projector's output does not jump onto the
//! operator's laptop screen; it waits, hidden, until the projector returns.

use std::collections::HashMap;

use lampstand_core::output::{DisplayMatch, OutputId};

/// One monitor as the window system reports it, in physical pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayInfo {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
}

impl DisplayInfo {
    /// What an output configured on this display remembers.
    pub fn to_match(&self) -> DisplayMatch {
        DisplayMatch {
            name: self.name.clone(),
            width: self.width,
            height: self.height,
            x: self.x,
            y: self.y,
        }
    }
}

/// Name, size and position matches, then minus the distance: compared in
/// that order, higher is better.
type Score = (u8, u8, u8, i64);

/// How well `display` fits `want`; `None` if it may not be used at all.
/// Higher is better: name beats size beats position, and among equals the
/// nearer position wins.
fn score(want: &DisplayMatch, display: &DisplayInfo) -> Option<Score> {
    let name = !want.name.is_empty() && want.name == display.name;
    let size = want.width == display.width && want.height == display.height;
    if !name && !size {
        return None;
    }
    let position = want.x == display.x && want.y == display.y;
    let distance =
        (want.x as i64 - display.x as i64).abs() + (want.y as i64 - display.y as i64).abs();
    Some((name as u8, size as u8, position as u8, -distance))
}

/// The best display for one output, ignoring the others.
pub fn match_display(want: &DisplayMatch, displays: &[DisplayInfo]) -> Option<usize> {
    displays
        .iter()
        .enumerate()
        .filter_map(|(i, d)| score(want, d).map(|s| (s, i)))
        .max_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)))
        .map(|(_, i)| i)
}

/// Gives each output at most one display and each display at most one
/// output, best matches first. Outputs with no acceptable display are left
/// out of the result.
pub fn assign_displays(
    wants: &[(OutputId, DisplayMatch)],
    displays: &[DisplayInfo],
) -> HashMap<OutputId, usize> {
    let mut candidates: Vec<(Score, usize, usize)> = Vec::new();
    for (o, (_, want)) in wants.iter().enumerate() {
        for (d, display) in displays.iter().enumerate() {
            if let Some(s) = score(want, display) {
                candidates.push((s, o, d));
            }
        }
    }
    // Best score first; ties go to the earlier output, then display.
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    let mut taken_output = vec![false; wants.len()];
    let mut taken_display = vec![false; displays.len()];
    let mut out = HashMap::new();
    for (_, o, d) in candidates {
        if taken_output[o] || taken_display[d] {
            continue;
        }
        taken_output[o] = true;
        taken_display[d] = true;
        out.insert(wants[o].0, d);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn display(name: &str, width: u32, height: u32, x: i32, y: i32) -> DisplayInfo {
        DisplayInfo {
            name: name.into(),
            width,
            height,
            x,
            y,
        }
    }

    fn want(name: &str, width: u32, height: u32, x: i32, y: i32) -> DisplayMatch {
        display(name, width, height, x, y).to_match()
    }

    #[test]
    fn name_wins_over_size_and_position() {
        let displays = [
            display("Built-in", 1920, 1080, 1920, 0),
            display("EPSON PJ", 1280, 800, 0, 0),
        ];
        // Same name, moved and at a new resolution: still the projector.
        assert_eq!(
            match_display(&want("EPSON PJ", 1920, 1080, 1920, 0), &displays),
            Some(1)
        );
    }

    #[test]
    fn size_is_used_when_the_name_changed() {
        let displays = [
            display("Built-in", 2560, 1600, 0, 0),
            display("HDMI-1", 1920, 1080, 2560, 0),
        ];
        assert_eq!(
            match_display(&want("DELL U2419H", 1920, 1080, 2560, 0), &displays),
            Some(1)
        );
    }

    #[test]
    fn position_breaks_ties() {
        let displays = [
            display("TV", 1920, 1080, 0, 0),
            display("TV", 1920, 1080, 1920, 0),
            display("TV", 1920, 1080, 3840, 0),
        ];
        assert_eq!(
            match_display(&want("TV", 1920, 1080, 1920, 0), &displays),
            Some(1)
        );
        // Nearest when none is exact.
        assert_eq!(
            match_display(&want("TV", 1920, 1080, 3000, 0), &displays),
            Some(2)
        );
    }

    #[test]
    fn a_missing_display_is_not_replaced_by_another() {
        let displays = [display("Built-in", 2560, 1600, 0, 0)];
        assert_eq!(
            match_display(&want("EPSON PJ", 1280, 800, 2560, 0), &displays),
            None
        );
    }

    #[test]
    fn assignment_uses_each_display_once() {
        let a = OutputId::generate();
        let b = OutputId::generate();
        let c = OutputId::generate();
        let displays = [
            display("TV", 1920, 1080, 0, 0),
            display("TV", 1920, 1080, 1920, 0),
        ];
        let wants = [
            (a, want("TV", 1920, 1080, 1920, 0)),
            (b, want("TV", 1920, 1080, 0, 0)),
            (c, want("TV", 1920, 1080, 0, 0)),
        ];
        let got = assign_displays(&wants, &displays);
        assert_eq!(got.get(&a), Some(&1));
        assert_eq!(got.get(&b), Some(&0));
        assert_eq!(got.get(&c), None, "no third display");
    }

    #[test]
    fn a_name_match_is_not_stolen_by_a_size_match() {
        let stage = OutputId::generate();
        let audience = OutputId::generate();
        let displays = [
            display("Stage TV", 1920, 1080, 0, 0),
            display("Projector", 1920, 1080, 1920, 0),
        ];
        // The stage output is listed first and matches the projector by
        // size and position, but the audience output owns it by name.
        let wants = [
            (stage, want("Old stage TV", 1920, 1080, 1920, 0)),
            (audience, want("Projector", 1920, 1080, 5000, 0)),
        ];
        let got = assign_displays(&wants, &displays);
        assert_eq!(got.get(&audience), Some(&1));
        assert_eq!(got.get(&stage), Some(&0));
    }

    #[test]
    fn unplugging_one_display_keeps_the_others() {
        let a = OutputId::generate();
        let b = OutputId::generate();
        let wants = [
            (a, want("Projector", 1920, 1080, 1920, 0)),
            (b, want("Stage TV", 1280, 720, 3840, 0)),
        ];
        let before = [
            display("Built-in", 2560, 1600, 0, 0),
            display("Projector", 1920, 1080, 1920, 0),
            display("Stage TV", 1280, 720, 3840, 0),
        ];
        let after = [before[0].clone(), before[2].clone()];
        assert_eq!(assign_displays(&wants, &before).len(), 2);
        let got = assign_displays(&wants, &after);
        assert_eq!(got.get(&a), None);
        assert_eq!(got.get(&b), Some(&1));
    }
}
