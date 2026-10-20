//! `p:timing` → build steps and `p:transition` → `Transition`, on
//! DeckCraft's `deckcraft-anim`.
//!
//! DeckCraft's reader flattens the timing tree into the Animation Pane
//! list, and its `Timeline` groups that list into click steps, folds "with
//! previous" and "after previous" into start times, and expands a
//! by-paragraph build into one effect per first-level paragraph (deeper
//! paragraphs follow theirs). Each effect becomes one `Animation` whose
//! delay is its start within the step.
//!
//! **Paragraphs.** Our text blocks have no per-paragraph node, so a text
//! block with paragraph builds becomes a group of copies of the block, one
//! per built paragraph (with the paragraphs that follow it) and one for the
//! rest, each showing only its own paragraphs: the others are there, laid
//! out the same, but transparent. Every copy wraps and places text exactly
//! as the whole block would, so each paragraph is its own node that a build
//! can fade, fly or wipe, and PowerPoint's rule that the text does not move
//! as paragraphs appear holds.
//!
//! **Steps.** When the first effect starts with or after the previous one,
//! it plays as the slide arrives: that step is `DeckSlide::entry`, and
//! `DeckSlide::builds` holds one step per click. A node whose first effect
//! (in the Animation Pane's order) is an entrance starts hidden.
//!
//! **Effects.** PowerPoint's effects map onto `scene::Effect`; ones with no
//! counterpart (shape and split reveals, colour emphasis) become the
//! closest one and are reported as approximated, and motion paths, colour
//! changes and media pauses are reported as unsupported and left out.
//! Sequences triggered by clicking a shape are left out and reported: the
//! operator advances with Next, not by clicking the projected slide.

use std::collections::{BTreeMap, BTreeSet};

use deckcraft_anim::{EffectTiming, Sequence, Timeline};
use deckcraft_model::anim::{animation_info, Transition as DcTransition};
use deckcraft_model::{AnimClass, Slide};
use lampstand_core::scene::{
    Animation, Direction, Easing, Effect, Node, NodeId, NodeKind, TextBlock, Transition,
    TransitionKind,
};

use super::{limits, SlideCx};
use crate::{BuildStep, IssueKind};

/// The slide's animations in time, from DeckCraft.
pub(crate) fn timeline(slide: &Slide, size: (f32, f32)) -> Timeline {
    Timeline::new(slide, f64::from(size.0), f64::from(size.1))
}

/// Paragraph groups each shape's text splits into, by shape ID: each group
/// is a built paragraph followed by the deeper paragraphs that build with
/// it, in order.
pub(crate) fn paragraph_groups(tl: &Timeline) -> BTreeMap<u32, Vec<Vec<usize>>> {
    let mut owners: BTreeMap<u32, BTreeSet<usize>> = BTreeMap::new();
    for e in tl.effects() {
        if let Some(p) = e.paragraph {
            owners.entry(e.shape.0).or_default().insert(p);
        }
    }
    let mut out = BTreeMap::new();
    for (shape, owners) in owners {
        let mut groups: Vec<Vec<usize>> = owners.iter().map(|&p| vec![p]).collect();
        for p in tl.paragraph_targets(deckcraft_model::ShapeId(shape)) {
            if owners.contains(&p) {
                continue;
            }
            // A deeper paragraph follows the built paragraph before it.
            if let Some(i) = owners
                .range(..p)
                .next_back()
                .and_then(|o| groups.iter().position(|g| g.first() == Some(o)))
            {
                groups[i].push(p);
            }
        }
        out.insert(shape, groups);
    }
    out
}

impl SlideCx<'_, '_> {
    /// A shape's text block as a node: split into one copy per paragraph
    /// group when the shape has paragraph builds (see the module comment).
    pub(crate) fn text_node(&mut self, shape: u32, block: TextBlock) -> NodeKind {
        let Some(groups) = self
            .para_groups
            .get(&shape)
            .cloned()
            .filter(|_| !self.backdrop)
        else {
            return NodeKind::Text(block);
        };
        let grouped: BTreeSet<usize> = groups.iter().flatten().copied().collect();
        let mut kids = Vec::with_capacity(groups.len() + 1);
        let rest: Vec<usize> = (0..block.paragraphs.len())
            .filter(|i| !grouped.contains(i))
            .collect();
        let has_text = |i: &usize| {
            block.paragraphs[*i]
                .runs
                .iter()
                .any(|r| !r.text.trim().is_empty())
        };
        if rest.iter().any(has_text) {
            let id = self.fresh_id();
            kids.push(self.copy_showing(id, &block, &rest));
        }
        for group in &groups {
            let Some(&owner) = group.first() else {
                continue;
            };
            if owner >= block.paragraphs.len() {
                continue;
            }
            let id = self.fresh_id();
            kids.push(self.copy_showing(id, &block, group));
            self.para_nodes.insert((shape, owner), id);
        }
        NodeKind::Group(kids)
    }

    /// `block` with every paragraph not in `shown` made transparent.
    fn copy_showing(&mut self, id: NodeId, block: &TextBlock, shown: &[usize]) -> Node {
        let mut copy = block.clone();
        for (i, p) in copy.paragraphs.iter_mut().enumerate() {
            if !shown.contains(&i) {
                for r in &mut p.runs {
                    r.color.a = 0;
                }
            }
            self.text_parts += 1 + p.runs.len();
        }
        super::shape::node(
            id,
            lampstand_core::geom::Transform::IDENTITY,
            NodeKind::Text(copy),
        )
    }
}

/// The slide's build steps: the one that plays as it arrives, then one per
/// click. Hides the nodes an entrance reveals.
pub(crate) fn steps(sc: &mut SlideCx, slide: &Slide, tl: &Timeline) -> (BuildStep, Vec<BuildStep>) {
    let mut ids = BTreeSet::new();
    collect_ids(&sc.nodes, &mut ids);
    let effects = tl.effects();
    if effects.iter().any(|e| e.sequence != Sequence::Main) {
        sc.issue(IssueKind::UnsupportedEffect {
            name: "an animation triggered by clicking a shape".into(),
        });
    }
    let main: Vec<&EffectTiming> = effects
        .iter()
        .filter(|e| e.sequence == Sequence::Main)
        .collect();
    let target = |sc: &SlideCx, e: &EffectTiming| -> Option<NodeId> {
        match e.paragraph {
            Some(p) => sc.para_nodes.get(&(e.shape.0, p)).copied(),
            None => Some(e.shape.0).filter(|id| ids.contains(id)),
        }
    };

    // A node starts hidden when its first effect is an entrance.
    let mut first: BTreeMap<NodeId, (usize, AnimClass)> = BTreeMap::new();
    for e in &main {
        if let Some(node) = target(sc, e) {
            let seen = first.entry(node).or_insert((e.anim, e.class));
            if e.anim < seen.0 {
                *seen = (e.anim, e.class);
            }
        }
    }
    let hidden: BTreeSet<NodeId> = first
        .into_iter()
        .filter(|(_, (_, class))| *class == AnimClass::Entrance)
        .map(|(id, _)| id)
        .collect();
    hide(&mut sc.nodes, &hidden);

    let auto = tl.step_is_auto(0);
    let mut entry = BuildStep::default();
    let clicks = tl.steps().saturating_sub(usize::from(auto));
    let mut builds = vec![BuildStep::default(); clicks];
    for e in main {
        let Some(a) = slide.animations.get(e.anim) else {
            continue;
        };
        let Some(node) = target(sc, e) else { continue };
        let Some(effect) = effect(sc, a) else {
            continue;
        };
        let animation = Animation {
            node,
            effect,
            delay_ms: millis(e.start),
            duration_ms: millis(e.duration),
            easing: easing(a),
        };
        let step = match (auto, e.step) {
            (true, 0) => Some(&mut entry),
            (true, s) => builds.get_mut(s - 1),
            (false, s) => builds.get_mut(s),
        };
        if let Some(step) = step {
            step.animations.push(animation);
        }
    }
    (entry, builds)
}

fn collect_ids(nodes: &[Node], out: &mut BTreeSet<NodeId>) {
    for n in nodes {
        out.insert(n.id);
        if let NodeKind::Group(kids) = &n.kind {
            collect_ids(kids, out);
        }
    }
}

fn hide(nodes: &mut [Node], ids: &BTreeSet<NodeId>) {
    for n in nodes {
        if ids.contains(&n.id) {
            n.visible = false;
        }
        if let NodeKind::Group(kids) = &mut n.kind {
            hide(kids, ids);
        }
    }
}

/// Seconds to whole milliseconds, within a minute (no real build is
/// longer, and a hostile one cannot hold the output for hours).
fn millis(seconds: f64) -> u32 {
    if !seconds.is_finite() {
        return 0;
    }
    (seconds * 1000.0).round().clamp(0.0, 60_000.0) as u32
}

/// PowerPoint's smooth start and end. Effects that move ease in and out by
/// default, as DeckCraft plays them.
fn easing(a: &deckcraft_model::Animation) -> Easing {
    let moves = matches!(
        a.effect.as_str(),
        "fly" | "flyOut" | "float" | "floatOut" | "rise" | "sinkDown" | "peek" | "peekOut"
    );
    match (a.smooth_start > 0.0, a.smooth_end > 0.0) {
        (true, true) => Easing::EaseInOut,
        (true, false) => Easing::EaseIn,
        (false, true) => Easing::EaseOut,
        (false, false) if moves => Easing::EaseInOut,
        (false, false) => Easing::Linear,
    }
}

/// The side an effect option names: `l`, `r`, `t`/`u`, `b`/`d`, with
/// diagonals taken by their side (`lb` is left).
fn side(option: &str, default: Direction) -> Direction {
    match option.chars().next() {
        Some('l') => Direction::Left,
        Some('r') => Direction::Right,
        Some('t' | 'u') => Direction::Up,
        Some('b' | 'd') => Direction::Down,
        _ => default,
    }
}

/// Our effect for a DeckCraft animation; approximations and effects left
/// out are reported.
fn effect(sc: &mut SlideCx, a: &deckcraft_model::Animation) -> Option<Effect> {
    let name =
        || animation_info(&a.effect, a.class).map_or_else(|| a.effect.clone(), |i| i.1.to_string());
    let exact = |e: Effect| Some((e, false));
    let approx = |e: Effect| Some((e, true));
    let o = a.option.as_str();
    let mapped = match a.class {
        AnimClass::Entrance => match a.effect.as_str() {
            "appear" => exact(Effect::Appear),
            "fade" => exact(Effect::FadeIn),
            "fly" => exact(Effect::FlyIn(side(o, Direction::Down))),
            "wipe" => exact(Effect::WipeIn(side(o, Direction::Down))),
            "zoom" | "expand" => exact(Effect::ZoomIn),
            "rise" => approx(Effect::FlyIn(Direction::Down)),
            "float" => approx(Effect::FadeIn),
            "peek" => approx(Effect::WipeIn(side(o, Direction::Down))),
            "grow" | "spinner" | "swivel" | "bounce" => approx(Effect::ZoomIn),
            _ => approx(Effect::FadeIn),
        },
        AnimClass::Exit => match a.effect.as_str() {
            "disappear" => exact(Effect::Disappear),
            "fadeOut" => exact(Effect::FadeOut),
            "flyOut" => exact(Effect::FlyOut(side(o, Direction::Down))),
            "wipeOut" => exact(Effect::WipeOut(side(o, Direction::Down))),
            "zoomOut" | "contract" => exact(Effect::ZoomOut),
            "sinkDown" => approx(Effect::FlyOut(Direction::Down)),
            "peekOut" => approx(Effect::WipeOut(side(o, Direction::Down))),
            "shrinkTurn" | "spinnerOut" | "swivelOut" | "bounceOut" => approx(Effect::ZoomOut),
            _ => approx(Effect::FadeOut),
        },
        AnimClass::Emphasis => {
            let amount = a.amount.filter(|v| v.is_finite());
            match a.effect.as_str() {
                "spin" => {
                    let turn = amount.unwrap_or(360.0).clamp(-3600.0, 3600.0) as f32;
                    let turn = if o == "counterClockwise" {
                        -turn.abs()
                    } else {
                        turn
                    };
                    exact(Effect::Spin(turn))
                }
                "growShrink" => exact(Effect::GrowShrink(
                    amount.unwrap_or(1.5).clamp(0.01, limits::MAX_SCALE) as f32,
                )),
                "pulse" => exact(Effect::Pulse),
                "transparency" => exact(Effect::Transparency(
                    amount.unwrap_or(0.5).clamp(0.0, 1.0) as f32,
                )),
                "teeter" | "wave" | "boldFlash" | "colorPulse" => approx(Effect::Pulse),
                _ => None,
            }
        }
        AnimClass::Media => match a.effect.as_str() {
            "play" => exact(Effect::Play),
            _ => None,
        },
        AnimClass::Path => None,
    };
    match mapped {
        Some((e, false)) => Some(e),
        Some((e, true)) => {
            sc.issue(IssueKind::ApproximatedEffect {
                name: name(),
                used: format!("{e:?}"),
            });
            Some(e)
        }
        None => {
            sc.issue(IssueKind::UnsupportedEffect { name: name() });
            None
        }
    }
}

/// How a slide arrives. PowerPoint's transition options name the way
/// things move (`l`: the new slide comes in from the right); ours name the
/// side it enters from. Transitions we don't draw become a fade, reported.
pub(crate) fn transition(sc: &mut SlideCx, t: Option<&DcTransition>) -> Transition {
    let Some(t) = t else {
        return Transition::CUT;
    };
    // Movement → the side it starts from.
    let from = |o: &str| match o.chars().next().unwrap_or('l') {
        'r' => Direction::Left,
        'u' => Direction::Down,
        'd' => Direction::Up,
        _ => Direction::Right,
    };
    let o = t.option.as_str();
    let (kind, approximated) = match t.kind.as_str() {
        "none" => (TransitionKind::Cut, false),
        "cut" if o == "throughBlack" => (TransitionKind::FadeThroughBlack, true),
        "cut" => (TransitionKind::Cut, false),
        "fade" if o == "throughBlack" => (TransitionKind::FadeThroughBlack, false),
        "fade" => (TransitionKind::Fade, false),
        "push" => (TransitionKind::Push(from(o)), false),
        "wipe" => (TransitionKind::Wipe(from(o)), o.len() > 1),
        "cover" => (TransitionKind::Cover(from(o)), o.len() > 1),
        "uncover" | "pan" => (TransitionKind::Push(from(o)), true),
        "zoom" => (TransitionKind::Zoom, o == "rotate"),
        _ => (TransitionKind::Fade, true),
    };
    if approximated {
        let label = deckcraft_model::anim::TRANSITIONS
            .iter()
            .find(|x| x.0 == t.kind)
            .map_or(t.kind.as_str(), |x| x.1);
        sc.issue(IssueKind::ApproximatedEffect {
            name: format!("{label} transition"),
            used: format!("{kind:?}"),
        });
    }
    Transition {
        kind,
        duration_ms: if kind == TransitionKind::Cut {
            0
        } else {
            t.duration_ms.min(10_000)
        },
    }
}
