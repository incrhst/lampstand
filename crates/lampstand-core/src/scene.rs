//! The scene format: what every renderer draws.
//!
//! `lampstand-slides` turns each PPTX slide into a `Scene` in slide points.
//! `lampstand-layout` composes the final `Scene` for one output, in that
//! output's pixels, wrapping any deck scene in a `Group` that fits it to the
//! safe area. `lampstand-render` (wgpu) and the OBS browser source (DOM)
//! draw it. Renderers do no layout of their own beyond shaping the text runs
//! they are given into the box they are given.
//!
//! Node IDs below `COMPOSITOR_NODE_BASE` belong to content (deck shapes, so
//! that build animations can target them); the compositor numbers its own
//! nodes from `COMPOSITOR_NODE_BASE` up.

use serde::{Deserialize, Serialize};

use crate::geom::{Color, Point, Rect, Transform};
use crate::id::BlobHash;

pub type NodeId = u32;

pub const COMPOSITOR_NODE_BASE: NodeId = 1 << 24;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    pub width: f32,
    pub height: f32,
    /// Painted from back to front.
    pub nodes: Vec<Node>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub transform: Transform,
    pub opacity: f32,
    /// False for shapes that a build animation will reveal.
    pub visible: bool,
    pub kind: NodeKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NodeKind {
    Group(Vec<Node>),
    Path(PathNode),
    Text(TextBlock),
    Image(ImageNode),
    Video(VideoNode),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PathNode {
    pub commands: Vec<PathCommand>,
    pub fill: Option<Paint>,
    pub stroke: Option<Stroke>,
    pub shadow: Option<Shadow>,
}

/// Preset PPTX geometries (roundRect, chevron...) are converted to paths by
/// the slide engine; renderers only ever see paths.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum PathCommand {
    MoveTo(Point),
    LineTo(Point),
    QuadTo(Point, Point),
    CubicTo(Point, Point, Point),
    Close,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Paint {
    Solid(Color),
    Linear {
        start: Point,
        end: Point,
        stops: Vec<GradientStop>,
    },
    Radial {
        center: Point,
        radius: f32,
        stops: Vec<GradientStop>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GradientStop {
    /// 0.0 to 1.0.
    pub offset: f32,
    pub color: Color,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    pub paint: Paint,
    pub width: f32,
    /// Dash and gap lengths, in scene units. `None` is a solid line.
    pub dash: Option<Vec<f32>>,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "automerge",
    derive(autosurgeon::Hydrate, autosurgeon::Reconcile)
)]
pub struct Shadow {
    pub color: Color,
    pub offset: Point,
    pub blur: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Fit {
    /// Stretch to the rect.
    Fill,
    /// Whole image visible, letterboxed.
    Contain,
    /// Rect fully covered, edges cropped.
    Cover,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageNode {
    pub blob: BlobHash,
    pub rect: Rect,
    pub fit: Fit,
    /// Source crop as fractions of the image (PPTX `srcRect`).
    pub crop: Option<Rect>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoNode {
    pub blob: BlobHash,
    pub rect: Rect,
    pub fit: Fit,
    pub looped: bool,
    pub muted: bool,
    /// Start playing when the node first appears (otherwise on a build step).
    pub autoplay: bool,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "automerge",
    derive(autosurgeon::Hydrate, autosurgeon::Reconcile)
)]
pub enum HAlign {
    Left,
    Center,
    Right,
    Justify,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "automerge",
    derive(autosurgeon::Hydrate, autosurgeon::Reconcile)
)]
pub enum VAlign {
    Top,
    Middle,
    Bottom,
}

/// Text already sized to fit: renderers shape and wrap the runs inside
/// `rect` and never change font sizes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextBlock {
    pub rect: Rect,
    pub valign: VAlign,
    pub paragraphs: Vec<Paragraph>,
    pub shadow: Option<Shadow>,
    pub outline: Option<Outline>,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "automerge",
    derive(autosurgeon::Hydrate, autosurgeon::Reconcile)
)]
pub struct Outline {
    pub color: Color,
    pub width: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Paragraph {
    pub align: HAlign,
    /// Line height as a multiple of the font size (1.2 = 120%). Layout
    /// (`fit_text`, pagination) and every renderer use this same rule.
    pub line_spacing: f32,
    pub space_before: f32,
    pub space_after: f32,
    pub indent: f32,
    /// Bullet glyph, already resolved ("•", "1.", "a)").
    pub bullet: Option<String>,
    pub runs: Vec<TextRun>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextRun {
    pub text: String,
    pub font_family: String,
    /// In scene units.
    pub size: f32,
    /// CSS weight, 100 to 900.
    pub weight: u16,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub color: Color,
}

/// How one scene replaces the previous one on an output.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "automerge",
    derive(autosurgeon::Hydrate, autosurgeon::Reconcile)
)]
pub struct Transition {
    pub kind: TransitionKind,
    pub duration_ms: u32,
}

impl Transition {
    pub const CUT: Self = Self {
        kind: TransitionKind::Cut,
        duration_ms: 0,
    };
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "automerge",
    derive(autosurgeon::Hydrate, autosurgeon::Reconcile)
)]
pub enum TransitionKind {
    Cut,
    Fade,
    FadeThroughBlack,
    Push(Direction),
    Wipe(Direction),
    Cover(Direction),
    Zoom,
}

/// Always a side of the screen (transitions) or of the shape (effects),
/// in PowerPoint's "From Left" sense: the side a push or cover enters
/// from, where a wipe starts, where a fly-in comes from and a fly-out
/// goes, where a wipe-out starts hiding.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "automerge",
    derive(autosurgeon::Hydrate, autosurgeon::Reconcile)
)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

/// One node's animation within a build step.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Animation {
    pub node: NodeId,
    pub effect: Effect,
    /// From the start of the build step (PPTX "with" and "after previous"
    /// timings are folded into this).
    pub delay_ms: u32,
    pub duration_ms: u32,
    pub easing: Easing,
}

/// The Phase 1 effect set: PowerPoint's common entrance, exit and emphasis
/// effects. Anything else maps to the closest one and is reported in the
/// fidelity report.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Effect {
    Appear,
    Disappear,
    FadeIn,
    FadeOut,
    FlyIn(Direction),
    FlyOut(Direction),
    WipeIn(Direction),
    WipeOut(Direction),
    ZoomIn,
    ZoomOut,
    /// Emphasis: rotate by this many degrees.
    Spin(f32),
    /// Emphasis: scale to this factor and stay.
    GrowShrink(f32),
    /// Emphasis: scale up briefly and back.
    Pulse,
    /// Emphasis: become this transparent (0.5 = half see-through, as
    /// PowerPoint shows it) and stay.
    Transparency(f32),
    /// Start a video node playing.
    Play,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Easing {
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
}
