//! Outputs: the screens and streams a service is shown on.
//!
//! PRD: LV-3, TS-1 to TS-9, OB-1, PS-7.
//!
//! Output configuration belongs to one machine (its screens, its room), so
//! it lives in that device's local settings and is never synced.

use serde::{Deserialize, Serialize};

use crate::deck::DeckId;
use crate::id::{BlobHash, Id};
use crate::safe_area::SafeArea;
use crate::theme::ThemeId;

pub type OutputId = Id<OutputConfig>;

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputConfig {
    pub id: OutputId,
    pub name: String,
    pub role: OutputRole,
    pub target: OutputTarget,
    pub safe_area: SafeArea,
    pub background: BackgroundMode,
    /// Overrides the service's theme for text on this output.
    pub theme: Option<ThemeId>,
    /// What this screen shows when nothing is on air. `None` uses the
    /// device's default (`DeviceSettings::idle`).
    #[serde(default)]
    pub idle: Option<IdleScreen>,
    pub enabled: bool,
}

/// What a screen shows when nothing is on air and neither Black nor Logo is
/// on. Black and Logo stay one key away and cover it.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IdleScreen {
    /// The theme's background and nothing else ("Plain").
    Blank,
    /// The church's logo and name, else the Lampstand lamp, centred on a
    /// plain background. What a screen shows until a church chooses
    /// something else.
    Default,
    /// A picture in the blob store; `name` is the file it came from.
    Image { blob: BlobHash, name: String },
    /// A video in the blob store. It loops, silent.
    Video { blob: BlobHash, name: String },
    /// One slide (counted from 0) of a deck, shown with its builds played.
    DeckSlide { deck: DeckId, slide: usize },
}

/// Which idle screen an output shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdlePlan<'a> {
    /// The theme's plain background.
    Plain,
    /// The default: the church's logo and name if it has set a logo, else
    /// the Lampstand lamp (`Idle::mark`).
    Mark,
    /// A picture, video or deck slide the church chose.
    Chosen(&'a IdleScreen),
}

/// The idle screen an output shows: its own setting, else the device's
/// default for all screens, else (nothing chosen yet) the default idle
/// screen.
pub fn idle_plan<'a>(output: &'a OutputConfig, default: Option<&'a IdleScreen>) -> IdlePlan<'a> {
    match output.idle.as_ref().or(default) {
        None | Some(IdleScreen::Default) => IdlePlan::Mark,
        Some(IdleScreen::Blank) => IdlePlan::Plain,
        Some(chosen) => IdlePlan::Chosen(chosen),
    }
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutputRole {
    /// What the congregation sees. Exactly one audience output is primary:
    /// songs and scripture are split into slides against it, and every
    /// other output shows the same slides.
    Audience { primary: bool },
    /// Confidence monitor and stage display: current and next slide,
    /// speaker notes, clock and timer.
    Stage,
    /// The live-stream graphics layer: transparent background, lyrics as a
    /// lower third (OB-1).
    Stream,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum OutputTarget {
    /// A fullscreen native window on a physical display.
    Display(DisplayMatch),
    /// No window: served to OBS as a browser source (OB-2).
    BrowserSource { width: u32, height: u32 },
    /// A normal window, for rehearsal and testing.
    Window { width: u32, height: u32 },
}

/// How a configured output finds its display again after a reboot or a
/// replug. Phase 1 matches on name, then size, then position; Phase 2 adds
/// the EDID hardware ID (TS-10).
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayMatch {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
}

/// TS-3: whether backgrounds fill the whole screen or stay inside the safe
/// area. Text always stays inside it.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackgroundMode {
    Bleed,
    Fit,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::safe_area::SafeArea;

    fn output(idle: Option<IdleScreen>) -> OutputConfig {
        OutputConfig {
            id: OutputId::generate(),
            name: "Main".into(),
            role: OutputRole::Audience { primary: true },
            target: OutputTarget::Window {
                width: 960,
                height: 540,
            },
            safe_area: SafeArea::default(),
            background: BackgroundMode::Bleed,
            theme: None,
            idle,
            enabled: true,
        }
    }

    fn image(name: &str) -> IdleScreen {
        IdleScreen::Image {
            blob: BlobHash::of(name.as_bytes()),
            name: name.into(),
        }
    }

    #[test]
    fn a_screen_uses_its_own_idle_before_the_default() {
        let default = image("default.png");
        let own = image("own.png");
        let o = output(Some(own.clone()));
        assert_eq!(idle_plan(&o, Some(&default)), IdlePlan::Chosen(&own));
    }

    #[test]
    fn a_screen_without_one_uses_the_default_for_all_screens() {
        let default = image("default.png");
        assert_eq!(
            idle_plan(&output(None), Some(&default)),
            IdlePlan::Chosen(&default)
        );
    }

    #[test]
    fn plain_on_a_screen_overrides_the_default() {
        let default = image("default.png");
        let o = output(Some(IdleScreen::Blank));
        assert_eq!(idle_plan(&o, Some(&default)), IdlePlan::Plain);
    }

    #[test]
    fn plain_for_all_screens_is_the_bare_theme_background() {
        assert_eq!(
            idle_plan(&output(None), Some(&IdleScreen::Blank)),
            IdlePlan::Plain
        );
    }

    #[test]
    fn a_screen_can_choose_the_default_over_a_picture_for_all_screens() {
        let default = image("default.png");
        let o = output(Some(IdleScreen::Default));
        assert_eq!(idle_plan(&o, Some(&default)), IdlePlan::Mark);
    }

    #[test]
    fn nothing_chosen_anywhere_is_the_default_idle_screen() {
        assert_eq!(idle_plan(&output(None), None), IdlePlan::Mark);
        assert_eq!(
            idle_plan(&output(None), Some(&IdleScreen::Default)),
            IdlePlan::Mark
        );
    }

    #[test]
    fn an_old_output_file_without_idle_still_loads() {
        let o = output(None);
        let mut json: serde_json::Value = serde_json::to_value(&o).unwrap();
        json.as_object_mut().unwrap().remove("idle");
        let back: OutputConfig = serde_json::from_value(json).unwrap();
        assert_eq!(back.idle, None);
    }
}
