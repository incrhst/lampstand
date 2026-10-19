//! Transitions and build animations as pure functions of time.
//!
//! Nothing here touches the GPU. The render thread asks "what does this
//! output look like `t` milliseconds in?" and draws the answer, so every
//! timeline is unit-tested without a device.
//!
//! **Directions.** `Direction` always names a side of the screen or shape:
//!
//! - `Push(d)`, `Cover(d)`: the incoming scene enters from side `d`.
//! - `Wipe(d)`: the incoming scene is revealed starting at side `d`.
//! - `FlyIn(d)`: the shape enters from side `d`; `FlyOut(d)` leaves
//!   towards side `d`.
//! - `WipeIn(d)`: the shape is revealed starting at side `d`; `WipeOut(d)`
//!   is hidden starting at side `d`.
//!
//! That is PowerPoint's "From Left" wording, which is what T30 maps from.

use lampstand_core::geom::Rect;
use lampstand_core::scene::{Animation, Direction, Easing, Effect, NodeId, TransitionKind};

/// Applies an easing curve to `t` in 0..=1.
pub fn ease(easing: Easing, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    match easing {
        Easing::Linear => t,
        Easing::EaseIn => t * t * t,
        Easing::EaseOut => 1.0 - (1.0 - t).powi(3),
        Easing::EaseInOut => {
            if t < 0.5 {
                4.0 * t * t * t
            } else {
                1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
            }
        }
    }
}

/// Fraction of `duration_ms` that `elapsed_ms` covers, in 0..=1. A zero
/// duration is complete at once.
pub fn progress(elapsed_ms: f32, duration_ms: u32) -> f32 {
    if duration_ms == 0 {
        1.0
    } else {
        (elapsed_ms / duration_ms as f32).clamp(0.0, 1.0)
    }
}

/// Which scene a composite layer draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    Outgoing,
    Incoming,
}

/// How a layer combines with what is under it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Blend {
    /// Premultiplied "over".
    Over,
    /// Premultiplied add: two layers with weights summing to 1 give an
    /// exact cross-fade, transparent areas included (stream outputs).
    Add,
}

/// One scene texture drawn onto the output.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompositeLayer {
    pub which: Which,
    /// Offset as a fraction of the output size (1.0 = one full width).
    pub offset: (f32, f32),
    /// Scale about the output's centre.
    pub scale: f32,
    pub opacity: f32,
    /// The part of the scene that is drawn, as fractions of it, before the
    /// offset and scale move it; `UNIT` draws it all.
    pub clip: Rect,
    pub blend: Blend,
}

pub const UNIT: Rect = Rect {
    x: 0.0,
    y: 0.0,
    width: 1.0,
    height: 1.0,
};

impl CompositeLayer {
    fn full(which: Which) -> Self {
        Self {
            which,
            offset: (0.0, 0.0),
            scale: 1.0,
            opacity: 1.0,
            clip: UNIT,
            blend: Blend::Over,
        }
    }
}

/// Unit vector pointing from the centre towards side `d`, in screen
/// coordinates (y down).
fn towards(d: Direction) -> (f32, f32) {
    match d {
        Direction::Left => (-1.0, 0.0),
        Direction::Right => (1.0, 0.0),
        Direction::Up => (0.0, -1.0),
        Direction::Down => (0.0, 1.0),
    }
}

/// The part of a unit box that is revealed when a wipe starting at side
/// `from` has covered fraction `p`.
pub fn wipe_rect(from: Direction, p: f32) -> Rect {
    let p = p.clamp(0.0, 1.0);
    match from {
        Direction::Left => Rect { width: p, ..UNIT },
        Direction::Right => Rect {
            x: 1.0 - p,
            width: p,
            ..UNIT
        },
        Direction::Up => Rect { height: p, ..UNIT },
        Direction::Down => Rect {
            y: 1.0 - p,
            height: p,
            ..UNIT
        },
    }
}

/// The part of a unit box still showing when a wipe-out starting at side
/// `from` has covered fraction `p`.
pub fn unwipe_rect(from: Direction, p: f32) -> Rect {
    let opposite = match from {
        Direction::Left => Direction::Right,
        Direction::Right => Direction::Left,
        Direction::Up => Direction::Down,
        Direction::Down => Direction::Up,
    };
    wipe_rect(opposite, 1.0 - p)
}

/// The layers to draw, back to front, `p` (0..=1, linear time) through a
/// transition. At `p = 1` every kind is just the incoming scene. The output
/// is cleared to its background (black, or transparent on a stream output)
/// first, so "through black" needs no black layer.
pub fn transition_layers(kind: TransitionKind, p: f32) -> Vec<CompositeLayer> {
    let p = p.clamp(0.0, 1.0);
    let incoming = CompositeLayer::full(Which::Incoming);
    let outgoing = CompositeLayer::full(Which::Outgoing);
    if p >= 1.0 {
        return vec![incoming];
    }
    let e = ease(Easing::EaseInOut, p);
    match kind {
        TransitionKind::Cut => vec![incoming],
        TransitionKind::Fade => vec![
            CompositeLayer {
                opacity: 1.0 - p,
                blend: Blend::Add,
                ..outgoing
            },
            CompositeLayer {
                opacity: p,
                blend: Blend::Add,
                ..incoming
            },
        ],
        TransitionKind::FadeThroughBlack => {
            if p < 0.5 {
                vec![CompositeLayer {
                    opacity: 1.0 - 2.0 * p,
                    ..outgoing
                }]
            } else {
                vec![CompositeLayer {
                    opacity: 2.0 * p - 1.0,
                    ..incoming
                }]
            }
        }
        TransitionKind::Push(from) => {
            // The incoming scene enters from `from`; both move away from it.
            let (dx, dy) = towards(from);
            vec![
                CompositeLayer {
                    offset: (-dx * e, -dy * e),
                    ..outgoing
                },
                CompositeLayer {
                    offset: (dx * (1.0 - e), dy * (1.0 - e)),
                    ..incoming
                },
            ]
        }
        TransitionKind::Cover(from) => {
            let (dx, dy) = towards(from);
            vec![
                outgoing,
                CompositeLayer {
                    offset: (dx * (1.0 - e), dy * (1.0 - e)),
                    ..incoming
                },
            ]
        }
        TransitionKind::Wipe(from) => vec![
            outgoing,
            CompositeLayer {
                clip: wipe_rect(from, e),
                ..incoming
            },
        ],
        TransitionKind::Zoom => vec![
            CompositeLayer {
                opacity: 1.0 - p,
                scale: 1.0 + 0.25 * e,
                blend: Blend::Add,
                ..outgoing
            },
            CompositeLayer {
                opacity: p,
                scale: 0.5 + 0.5 * e,
                blend: Blend::Add,
                ..incoming
            },
        ],
    }
}

/// What the running build animations do to one node, relative to how the
/// scene describes it. The renderer turns the relative parts (pivot, fly
/// distance, wipe box) into pixels using the node's bounds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NodeAnim {
    /// Overrides `Node::visible` when set.
    pub visible: Option<bool>,
    /// Multiplies the node's opacity.
    pub opacity: f32,
    /// Scale about the centre of the node's bounds.
    pub scale: f32,
    /// Clockwise rotation about the same centre, in degrees.
    pub rotation: f32,
    /// Moves the node towards a side of the output: 0 is where the scene
    /// puts it, 1 is just past that edge.
    pub fly: Option<(Direction, f32)>,
    /// The part of the node's bounds that is drawn, as fractions of them.
    pub clip: Option<Rect>,
}

impl Default for NodeAnim {
    fn default() -> Self {
        Self {
            visible: None,
            opacity: 1.0,
            scale: 1.0,
            rotation: 0.0,
            fly: None,
            clip: None,
        }
    }
}

impl NodeAnim {
    /// True when drawing the node needs no change at all.
    pub fn is_identity(&self) -> bool {
        *self == Self::default()
    }

    /// True when the node's transform changes.
    pub fn moves(&self) -> bool {
        self.scale != 1.0 || self.rotation != 0.0 || self.fly.is_some()
    }

    /// Folds one animation in, `elapsed_ms` after its build step started.
    fn apply(&mut self, a: &Animation, elapsed_ms: f32) {
        let local = elapsed_ms - a.delay_ms as f32;
        let started = local >= 0.0;
        let p = if started {
            progress(local, a.duration_ms)
        } else {
            0.0
        };
        let e = ease(a.easing, p);
        let entrance = matches!(
            a.effect,
            Effect::Appear | Effect::FadeIn | Effect::FlyIn(_) | Effect::WipeIn(_) | Effect::ZoomIn
        );
        if entrance && !started {
            // Entrance targets stay hidden until their own delay passes,
            // even if the scene already shows them.
            self.visible = Some(false);
            return;
        }
        if !started {
            return;
        }
        let exit_done = p >= 1.0;
        match a.effect {
            Effect::Appear => self.visible = Some(true),
            Effect::Disappear => self.visible = Some(false),
            Effect::FadeIn => {
                self.visible = Some(true);
                self.opacity *= e;
            }
            Effect::FadeOut => {
                self.visible = Some(!exit_done);
                self.opacity *= 1.0 - e;
            }
            Effect::FlyIn(from) => {
                self.visible = Some(true);
                if e < 1.0 {
                    self.fly = Some((from, 1.0 - e));
                }
            }
            Effect::FlyOut(to) => {
                self.visible = Some(!exit_done);
                self.fly = Some((to, e));
            }
            Effect::WipeIn(from) => {
                self.visible = Some(true);
                if e < 1.0 {
                    self.clip = Some(intersect(self.clip, wipe_rect(from, e)));
                }
            }
            Effect::WipeOut(from) => {
                self.visible = Some(!exit_done);
                self.clip = Some(intersect(self.clip, unwipe_rect(from, e)));
            }
            Effect::ZoomIn => {
                self.visible = Some(true);
                self.scale *= e;
                self.opacity *= e;
            }
            Effect::ZoomOut => {
                self.visible = Some(!exit_done);
                self.scale *= 1.0 - e;
                self.opacity *= 1.0 - e;
            }
            Effect::Spin(degrees) => self.rotation += degrees * e,
            Effect::GrowShrink(factor) => self.scale *= 1.0 + (factor - 1.0) * e,
            Effect::Pulse => self.scale *= 1.0 + 0.1 * (std::f32::consts::PI * p).sin(),
            // The value is the transparency PowerPoint shows (0.5 = 50%
            // see-through), so opacity goes to `1 - value`.
            Effect::Transparency(value) => {
                self.opacity *= 1.0 - value.clamp(0.0, 1.0) * e;
            }
            // Video playback is T27's.
            Effect::Play => {}
        }
    }
}

fn intersect(a: Option<Rect>, b: Rect) -> Rect {
    let Some(a) = a else { return b };
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = (a.x + a.width).min(b.x + b.width);
    let y1 = (a.y + a.height).min(b.y + b.height);
    Rect {
        x: x0,
        y: y0,
        width: (x1 - x0).max(0.0),
        height: (y1 - y0).max(0.0),
    }
}

/// Build steps run on one scene, oldest first, each with the time it
/// started (ms on any clock shared with `now`; f64 so hours of uptime keep
/// sub-millisecond precision).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AnimationTimeline {
    steps: Vec<(f64, Vec<Animation>)>,
}

impl AnimationTimeline {
    /// Starts a build step at `now`. A step still running is finished at
    /// once, as PowerPoint does when the presenter clicks through it.
    pub fn push(&mut self, now: f64, animations: Vec<Animation>) {
        for (start, step) in &mut self.steps {
            let length = step_length(step) as f64;
            if *start + length > now {
                *start = now - length;
            }
        }
        self.steps.push((now, animations));
    }

    /// Finishes every step at once (after a render-thread restart).
    pub fn finish_all(&mut self) {
        for (start, _) in &mut self.steps {
            *start = -1.0e12;
        }
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// True while any step is still changing.
    pub fn running(&self, now: f64) -> bool {
        self.steps
            .iter()
            .any(|(start, step)| now < start + step_length(step) as f64)
    }

    /// Every node's state at `now`. Nodes no animation touches are absent.
    pub fn state(&self, now: f64) -> std::collections::HashMap<NodeId, NodeAnim> {
        let mut out = std::collections::HashMap::new();
        for (start, step) in &self.steps {
            // Within a step, effects fold in start order, so a later effect
            // on the same node wins visibility and stacks emphasis.
            let mut ordered: Vec<&Animation> = step.iter().collect();
            ordered.sort_by_key(|a| a.delay_ms);
            for a in ordered {
                if matches!(a.effect, Effect::Play) {
                    continue;
                }
                out.entry(a.node)
                    .or_insert_with(NodeAnim::default)
                    .apply(a, (now - start) as f32);
            }
        }
        out.retain(|_, anim| !anim.is_identity());
        out
    }
}

/// When the last animation of a step ends, in ms from the step's start.
pub fn step_length(step: &[Animation]) -> f32 {
    step.iter()
        .map(|a| a.delay_ms as f32 + a.duration_ms as f32)
        .fold(0.0, f32::max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lampstand_core::scene::{Direction::*, Transition};

    fn anim(node: NodeId, effect: Effect, delay_ms: u32, duration_ms: u32) -> Animation {
        Animation {
            node,
            effect,
            delay_ms,
            duration_ms,
            easing: Easing::Linear,
        }
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn easings_start_at_zero_and_end_at_one() {
        for e in [
            Easing::Linear,
            Easing::EaseIn,
            Easing::EaseOut,
            Easing::EaseInOut,
        ] {
            assert_eq!(ease(e, 0.0), 0.0);
            assert!(close(ease(e, 1.0), 1.0));
            assert!(close(ease(e, 2.0), 1.0), "clamped above");
            assert_eq!(ease(e, -1.0), 0.0, "clamped below");
            let mut last = 0.0;
            for i in 1..=100 {
                let v = ease(e, i as f32 / 100.0);
                assert!(v >= last, "{e:?} is monotonic");
                last = v;
            }
        }
        assert!(ease(Easing::EaseIn, 0.5) < 0.5);
        assert!(ease(Easing::EaseOut, 0.5) > 0.5);
        assert!(close(ease(Easing::EaseInOut, 0.5), 0.5));
    }

    #[test]
    fn zero_duration_is_complete_at_once() {
        assert_eq!(progress(0.0, 0), 1.0);
        assert_eq!(progress(50.0, 100), 0.5);
        assert_eq!(progress(500.0, 100), 1.0);
        assert_eq!(Transition::CUT.duration_ms, 0);
    }

    #[test]
    fn every_transition_ends_on_the_incoming_scene_alone() {
        let kinds = [
            TransitionKind::Cut,
            TransitionKind::Fade,
            TransitionKind::FadeThroughBlack,
            TransitionKind::Push(Left),
            TransitionKind::Wipe(Up),
            TransitionKind::Cover(Down),
            TransitionKind::Zoom,
        ];
        for kind in kinds {
            assert_eq!(
                transition_layers(kind, 1.0),
                vec![CompositeLayer::full(Which::Incoming)],
                "{kind:?}"
            );
        }
        assert_eq!(
            transition_layers(TransitionKind::Cut, 0.0),
            vec![CompositeLayer::full(Which::Incoming)]
        );
    }

    #[test]
    fn transitions_start_on_the_outgoing_scene() {
        // At p = 0 the picture is exactly the outgoing scene.
        for kind in [
            TransitionKind::Fade,
            TransitionKind::FadeThroughBlack,
            TransitionKind::Push(Right),
            TransitionKind::Wipe(Left),
            TransitionKind::Cover(Up),
            TransitionKind::Zoom,
        ] {
            let layers = transition_layers(kind, 0.0);
            let out = layers.iter().find(|l| l.which == Which::Outgoing).unwrap();
            assert_eq!(out.opacity, 1.0, "{kind:?}");
            assert_eq!(out.offset, (0.0, 0.0), "{kind:?}");
            assert_eq!(out.scale, 1.0, "{kind:?}");
            for l in layers.iter().filter(|l| l.which == Which::Incoming) {
                let hidden = l.opacity == 0.0
                    || l.clip.width == 0.0
                    || l.clip.height == 0.0
                    || l.offset.0.abs() >= 1.0
                    || l.offset.1.abs() >= 1.0;
                assert!(hidden, "{kind:?} incoming visible at p=0: {l:?}");
            }
        }
    }

    #[test]
    fn fade_weights_sum_to_one() {
        for i in 0..10 {
            let p = i as f32 / 10.0;
            let layers = transition_layers(TransitionKind::Fade, p);
            assert_eq!(layers.len(), 2);
            assert!(layers.iter().all(|l| l.blend == Blend::Add));
            assert!(close(layers[0].opacity + layers[1].opacity, 1.0));
            assert_eq!(layers[1].which, Which::Incoming);
            assert!(close(layers[1].opacity, p));
        }
    }

    #[test]
    fn fade_through_black_is_dark_at_the_midpoint() {
        let mid = transition_layers(TransitionKind::FadeThroughBlack, 0.5);
        assert_eq!(mid.len(), 1);
        assert!(close(mid[0].opacity, 0.0));
        let early = transition_layers(TransitionKind::FadeThroughBlack, 0.25);
        assert_eq!(early[0].which, Which::Outgoing);
        assert!(close(early[0].opacity, 0.5));
        let late = transition_layers(TransitionKind::FadeThroughBlack, 0.75);
        assert_eq!(late[0].which, Which::Incoming);
        assert!(close(late[0].opacity, 0.5));
    }

    #[test]
    fn push_moves_both_scenes_away_from_the_entry_side() {
        let layers = transition_layers(TransitionKind::Push(Right), 0.5);
        let out = layers[0];
        let inc = layers[1];
        assert_eq!(out.which, Which::Outgoing);
        // Incoming enters from the right, so everything moves left, and the
        // two stay exactly one screen apart.
        assert!(out.offset.0 < 0.0);
        assert!(inc.offset.0 > 0.0);
        assert!(close(inc.offset.0 - out.offset.0, 1.0));
        assert_eq!(out.offset.1, 0.0);
        let up = transition_layers(TransitionKind::Push(Up), 0.1);
        assert!(up[1].offset.1 < 0.0, "enters from the top");
    }

    #[test]
    fn cover_leaves_the_outgoing_scene_still() {
        for p in [0.1, 0.5, 0.9] {
            let layers = transition_layers(TransitionKind::Cover(Left), p);
            assert_eq!(layers[0], CompositeLayer::full(Which::Outgoing));
            assert!(layers[1].offset.0 < 0.0, "enters from the left");
        }
    }

    #[test]
    fn wipe_reveals_from_the_named_side() {
        let layers = transition_layers(TransitionKind::Wipe(Left), 0.5);
        let clip = layers[1].clip;
        assert_eq!(clip.x, 0.0);
        assert!(close(clip.width, 0.5));
        let clip = transition_layers(TransitionKind::Wipe(Down), 0.5)[1].clip;
        assert!(close(clip.y, 0.5));
        assert!(close(clip.height, 0.5));
        assert_eq!(clip.width, 1.0);
    }

    #[test]
    fn zoom_grows_the_incoming_scene() {
        let a = transition_layers(TransitionKind::Zoom, 0.2)[1];
        let b = transition_layers(TransitionKind::Zoom, 0.8)[1];
        assert!(a.scale < b.scale && b.scale < 1.0);
        assert!(a.opacity < b.opacity);
    }

    #[test]
    fn wipe_rects_cover_and_uncover() {
        for d in [Left, Right, Up, Down] {
            assert_eq!(wipe_rect(d, 1.0), UNIT);
            let r = wipe_rect(d, 0.0);
            assert!(r.width == 0.0 || r.height == 0.0);
            assert_eq!(unwipe_rect(d, 0.0), UNIT);
            let r = unwipe_rect(d, 1.0);
            assert!(r.width == 0.0 || r.height == 0.0);
        }
        // Wiping out from the left removes the left part first.
        let r = unwipe_rect(Left, 0.25);
        assert!(close(r.x, 0.25) && close(r.width, 0.75));
    }

    fn state_at(t: &AnimationTimeline, now: f64, node: NodeId) -> NodeAnim {
        t.state(now).get(&node).copied().unwrap_or_default()
    }

    #[test]
    fn entrance_hides_until_its_delay_then_animates() {
        let mut t = AnimationTimeline::default();
        t.push(1000.0, vec![anim(7, Effect::FadeIn, 200, 400)]);
        assert_eq!(state_at(&t, 1100.0, 7).visible, Some(false));
        let mid = state_at(&t, 1400.0, 7);
        assert_eq!(mid.visible, Some(true));
        assert!(close(mid.opacity, 0.5));
        let end = state_at(&t, 1600.0, 7);
        assert_eq!(end.visible, Some(true));
        assert_eq!(end.opacity, 1.0);
        assert!(t.running(1599.0));
        assert!(!t.running(1600.0));
    }

    #[test]
    fn appear_and_disappear_switch_at_their_delay() {
        let mut t = AnimationTimeline::default();
        t.push(
            0.0,
            vec![
                anim(1, Effect::Appear, 100, 0),
                anim(2, Effect::Disappear, 100, 0),
            ],
        );
        assert_eq!(state_at(&t, 50.0, 1).visible, Some(false));
        assert_eq!(state_at(&t, 50.0, 2).visible, None);
        assert_eq!(state_at(&t, 100.0, 1).visible, Some(true));
        assert_eq!(state_at(&t, 100.0, 2).visible, Some(false));
    }

    #[test]
    fn exits_end_hidden() {
        for effect in [
            Effect::FadeOut,
            Effect::FlyOut(Left),
            Effect::WipeOut(Up),
            Effect::ZoomOut,
        ] {
            let mut t = AnimationTimeline::default();
            t.push(0.0, vec![anim(3, effect, 0, 500)]);
            let mid = state_at(&t, 250.0, 3);
            assert_eq!(mid.visible, Some(true), "{effect:?}");
            assert_eq!(state_at(&t, 500.0, 3).visible, Some(false), "{effect:?}");
        }
    }

    #[test]
    fn fly_in_travels_from_off_screen_to_rest() {
        let mut t = AnimationTimeline::default();
        t.push(0.0, vec![anim(4, Effect::FlyIn(Down), 0, 1000)]);
        assert_eq!(state_at(&t, 0.0, 4).fly, Some((Down, 1.0)));
        assert_eq!(state_at(&t, 250.0, 4).fly, Some((Down, 0.75)));
        let end = state_at(&t, 1000.0, 4);
        assert_eq!(end.fly, None);
        assert!(!end.moves());
    }

    #[test]
    fn wipe_in_reveals_progressively() {
        let mut t = AnimationTimeline::default();
        t.push(0.0, vec![anim(5, Effect::WipeIn(Right), 0, 100)]);
        let clip = state_at(&t, 40.0, 5).clip.unwrap();
        assert!(close(clip.x, 0.6) && close(clip.width, 0.4));
        assert_eq!(state_at(&t, 100.0, 5).clip, None);
    }

    #[test]
    fn emphasis_effects_stay_and_stack() {
        let mut t = AnimationTimeline::default();
        t.push(0.0, vec![anim(6, Effect::Spin(90.0), 0, 100)]);
        assert!(close(state_at(&t, 50.0, 6).rotation, 45.0));
        t.push(200.0, vec![anim(6, Effect::Spin(90.0), 0, 100)]);
        assert!(close(state_at(&t, 300.0, 6).rotation, 180.0));

        let mut t = AnimationTimeline::default();
        t.push(0.0, vec![anim(6, Effect::GrowShrink(1.5), 0, 100)]);
        assert!(close(state_at(&t, 100.0, 6).scale, 1.5));
        assert!(close(state_at(&t, 5000.0, 6).scale, 1.5));

        let mut t = AnimationTimeline::default();
        t.push(0.0, vec![anim(6, Effect::Transparency(0.75), 0, 100)]);
        assert!(close(state_at(&t, 100.0, 6).opacity, 0.25));
    }

    #[test]
    fn pulse_returns_to_normal_size() {
        let mut t = AnimationTimeline::default();
        t.push(0.0, vec![anim(8, Effect::Pulse, 0, 100)]);
        assert!(state_at(&t, 50.0, 8).scale > 1.05);
        assert!(close(state_at(&t, 100.0, 8).scale, 1.0));
        assert!(t.state(100.0).get(&8).is_none_or(|a| close(a.scale, 1.0)));
    }

    #[test]
    fn zoom_in_grows_from_nothing() {
        let mut t = AnimationTimeline::default();
        t.push(0.0, vec![anim(9, Effect::ZoomIn, 0, 100)]);
        let early = state_at(&t, 10.0, 9);
        assert!(early.scale < 0.2 && early.opacity < 0.2);
        assert!(t.state(100.0).get(&9).is_none_or(|a| !a.moves()));
    }

    #[test]
    fn play_is_ignored_here() {
        let mut t = AnimationTimeline::default();
        t.push(0.0, vec![anim(10, Effect::Play, 0, 100)]);
        assert!(t.state(50.0).is_empty());
    }

    #[test]
    fn a_new_step_finishes_the_running_one() {
        let mut t = AnimationTimeline::default();
        t.push(0.0, vec![anim(1, Effect::FadeIn, 0, 1000)]);
        assert!(close(state_at(&t, 100.0, 1).opacity, 0.1));
        t.push(100.0, vec![anim(2, Effect::FadeIn, 0, 1000)]);
        assert_eq!(state_at(&t, 100.0, 1).opacity, 1.0);
        assert_eq!(state_at(&t, 100.0, 1).visible, Some(true));
        assert!(close(state_at(&t, 600.0, 2).opacity, 0.5));
    }

    #[test]
    fn finish_all_jumps_to_the_end_state() {
        let mut t = AnimationTimeline::default();
        t.push(0.0, vec![anim(1, Effect::FlyIn(Left), 500, 1000)]);
        t.finish_all();
        let s = state_at(&t, 0.0, 1);
        assert_eq!(s.visible, Some(true));
        assert!(!s.moves());
        assert!(!t.running(0.0));
    }

    #[test]
    fn later_effects_on_a_node_win_visibility() {
        let mut t = AnimationTimeline::default();
        t.push(
            0.0,
            vec![
                anim(1, Effect::FadeOut, 500, 100),
                anim(1, Effect::FadeIn, 0, 100),
            ],
        );
        assert_eq!(state_at(&t, 200.0, 1).visible, Some(true));
        assert_eq!(state_at(&t, 700.0, 1).visible, Some(false));
    }
}
