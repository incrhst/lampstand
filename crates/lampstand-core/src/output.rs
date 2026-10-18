//! Outputs: the screens and streams a service is shown on.
//!
//! PRD: LV-3, TS-1 to TS-9, OB-1, PS-7.
//!
//! Output configuration belongs to one machine (its screens, its room), so
//! it lives in that device's local settings and is never synced.

use serde::{Deserialize, Serialize};

use crate::id::Id;
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
    pub enabled: bool,
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
