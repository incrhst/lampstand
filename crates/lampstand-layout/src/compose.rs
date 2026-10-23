//! Composing what is on air into one output's scene (T23).

use cosmic_text::{Attrs, Buffer, Family, Metrics, Shaping, Style, Weight, Wrap};
use lampstand_core::brand::bug_rect;
use lampstand_core::geom::{Color, Point, Rect, Transform};
use lampstand_core::id::Timestamp;
use lampstand_core::output::{BackgroundMode, OutputConfig, OutputRole};
use lampstand_core::program::{Content, Idle, Overlay, Program, SlideText};
use lampstand_core::scene::{
    Fit, HAlign, ImageNode, Node, NodeId, NodeKind, Paint, Paragraph, PathCommand, PathNode, Scene,
    Stroke, TextBlock, TextRun, VAlign, VideoNode, COMPOSITOR_NODE_BASE,
};
use lampstand_core::theme::{Background, TextStyle, Theme, REFERENCE_HEIGHT};

use crate::paginate::FOOTER_SCALE;
use crate::{fit_text, Fonts};

/// Output pixel size of an output, for composition.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OutputSize {
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Composed {
    pub scene: Scene,
    pub warnings: Vec<FitWarning>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FitWarning {
    BelowMinimum { size: f32 },
}

/// Draws what is on air for one output:
/// - Audience: background (bleeding or fitted per `BackgroundMode`), text
///   in the theme's text area inside the safe area, deck slides fitted to
///   the safe area keeping aspect ratio, overlays on top.
/// - Stage: current slide (deck thumbnail or text), next slide, notes,
///   clock and countdown, in a fixed high-contrast layout.
/// - Stream: transparent background, text as a lower third, deck slides
///   full frame.
///
/// Theme sizes scale by `size.height / REFERENCE_HEIGHT`.
///
/// Text is laid out in the safe area and nothing else. The footer is
/// [`FOOTER_SCALE`] of the preferred size with half a footer line above it,
/// the same as pagination reserves. The stage clock and countdown read the
/// system clock, in UTC; the app uses [`compose_at`] to pass the time and
/// the local UTC offset, which also keeps tests deterministic.
///
/// Overlays: `Black` and `Logo` cover an audience output completely. The
/// stage ignores them (the confidence monitor keeps its slides), and the
/// stream shows nothing under them, staying transparent. The logo screen is the
/// church's logo on its brand colour, or a ring until it has set one.
///
/// Logo bug: audience and stream outputs draw the church's logo small in a
/// lower corner of the safe area, last so it is over the slide and media. It
/// is not drawn under `Black` or `Logo`, nor on an idle screen that is the
/// logo itself, and the stage never has it. It has no animation of its own,
/// so it stays put across slide changes.
///
/// Idle: when the program carries an `Idle` (an audience output with
/// nothing on air), that picture, video or slide fills the screen in place
/// of the theme background. Stage and stream outputs never show it.
pub fn compose(
    program: &Program,
    output: &OutputConfig,
    size: OutputSize,
    theme: &Theme,
    fonts: &mut Fonts,
) -> Composed {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64);
    compose_at(program, output, size, theme, fonts, Timestamp(now), 0)
}

/// [`compose`] with the time given: `now`, and the offset of local time
/// from UTC in minutes for the stage clock.
pub fn compose_at(
    program: &Program,
    output: &OutputConfig,
    size: OutputSize,
    theme: &Theme,
    fonts: &mut Fonts,
    now: Timestamp,
    utc_offset_minutes: i32,
) -> Composed {
    let mut c = Compositor {
        program,
        output,
        size,
        theme,
        fonts,
        now,
        utc_offset_minutes,
        next_id: COMPOSITOR_NODE_BASE,
        warnings: Vec::new(),
        scale: size.height / REFERENCE_HEIGHT,
        safe: output.safe_area.rect(size.width, size.height),
    };
    let nodes = match output.role {
        OutputRole::Audience { .. } => {
            let nodes = c.audience();
            c.with_bug(nodes)
        }
        OutputRole::Stage => c.stage(),
        OutputRole::Stream => {
            let nodes = c.stream();
            c.with_bug(nodes)
        }
    };
    Composed {
        scene: Scene {
            width: size.width,
            height: size.height,
            nodes,
        },
        warnings: c.warnings,
    }
}

struct Compositor<'a> {
    program: &'a Program,
    output: &'a OutputConfig,
    size: OutputSize,
    theme: &'a Theme,
    fonts: &'a mut Fonts,
    now: Timestamp,
    utc_offset_minutes: i32,
    next_id: NodeId,
    warnings: Vec<FitWarning>,
    scale: f32,
    safe: Rect,
}

/// Slide text for a content item, or what to draw instead.
enum Shown {
    Text(SlideText),
    Deck(std::sync::Arc<Scene>),
    /// Image or video: drawn as media where there is room, else named.
    Nothing,
}

impl Compositor<'_> {
    fn id(&mut self) -> NodeId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn node(&mut self, kind: NodeKind) -> Node {
        Node {
            id: self.id(),
            transform: Transform::IDENTITY,
            opacity: 1.0,
            visible: true,
            kind,
        }
    }

    fn fill_rect(&mut self, rect: Rect, color: Color) -> Node {
        let path = PathNode {
            commands: rect_path(rect),
            fill: Some(Paint::Solid(color)),
            stroke: None,
            shadow: None,
        };
        self.node(NodeKind::Path(path))
    }

    fn full(&self) -> Rect {
        Rect {
            x: 0.0,
            y: 0.0,
            width: self.size.width,
            height: self.size.height,
        }
    }

    fn rect_in_safe(&self, f: Rect) -> Rect {
        Rect {
            x: self.safe.x + self.safe.width * f.x,
            y: self.safe.y + self.safe.height * f.y,
            width: self.safe.width * f.width,
            height: self.safe.height * f.height,
        }
    }

    fn describe(&self, content: &Content) -> Shown {
        match content {
            Content::Lyrics { slide, .. }
            | Content::Scripture { slide }
            | Content::Text { slide } => Shown::Text(slide.clone()),
            Content::DeckSlide { scene, .. } => Shown::Deck(scene.clone()),
            Content::Countdown { target, label } => Shown::Text(SlideText {
                label: None,
                lines: vec![format_countdown(target.0 - self.now.0)],
                footer: (!label.is_empty()).then(|| label.clone()),
            }),
            Content::Image { .. } | Content::Video { .. } => Shown::Nothing,
        }
    }

    /// Full-frame image or video content in `rect`, contained.
    fn media(&mut self, content: &Content, rect: Rect) -> Option<Node> {
        match content {
            Content::Image { blob } => Some(self.node(NodeKind::Image(ImageNode {
                blob: *blob,
                rect,
                fit: Fit::Contain,
                crop: None,
            }))),
            Content::Video { blob, looped } => Some(self.node(NodeKind::Video(VideoNode {
                blob: *blob,
                rect,
                fit: Fit::Contain,
                looped: *looped,
                muted: false,
                autoplay: true,
            }))),
            _ => None,
        }
    }

    /// A deck scene in slide points, in a group that fits `target` keeping
    /// its aspect ratio, centred. Deck node IDs are kept.
    fn deck_group(&mut self, scene: &Scene, target: Rect) -> Node {
        let aspect = scene.width / scene.height;
        let (w, h) = if target.width / target.height > aspect {
            (target.height * aspect, target.height)
        } else {
            (target.width, target.width / aspect)
        };
        let fitted = Rect {
            x: target.x + (target.width - w) / 2.0,
            y: target.y + (target.height - h) / 2.0,
            width: w,
            height: h,
        };
        let from = Rect {
            x: 0.0,
            y: 0.0,
            width: scene.width,
            height: scene.height,
        };
        let mut group = self.node(NodeKind::Group(scene.nodes.clone()));
        group.transform = Transform::rect_to_rect(from, fitted);
        group
    }

    fn audience(&mut self) -> Vec<Node> {
        let mut nodes = Vec::new();
        let black = self.fill_rect(self.full(), Color::BLACK);
        nodes.push(black);
        let content = self.program.content.as_ref();

        match self.program.overlay {
            Overlay::Black => return nodes,
            Overlay::Logo => {
                nodes.extend(self.logo_screen());
                return nodes;
            }
            Overlay::None => {}
        }

        // Nothing on air: the screen's idle picture, video or slide, in
        // place of the theme's background.
        if let Some(idle) = self.program.idle.clone() {
            nodes.extend(self.idle(&idle));
            return nodes;
        }

        let bg_rect = match self.output.background {
            BackgroundMode::Bleed => self.full(),
            BackgroundMode::Fit => self.safe,
        };
        let background = self
            .program
            .background
            .clone()
            .unwrap_or_else(|| self.theme.background.clone());
        nodes.extend(self.background(&background, bg_rect));

        let Some(content) = content else {
            return nodes;
        };
        if let Some(media) = self.media(content, self.safe) {
            nodes.push(media);
            return nodes;
        }
        match self.describe(content) {
            Shown::Deck(scene) => {
                let group = self.deck_group(&scene, self.safe);
                nodes.push(group);
            }
            Shown::Text(slide) => {
                let area = self.rect_in_safe(self.theme.text_area);
                nodes.extend(self.slide_text(&slide, area, true));
            }
            Shown::Nothing => {}
        }
        nodes
    }

    fn stream(&mut self) -> Vec<Node> {
        let mut nodes = Vec::new();
        if self.program.overlay != Overlay::None {
            return nodes;
        }
        let Some(content) = self.program.content.as_ref() else {
            return nodes;
        };
        if let Some(media) = self.media(content, self.full()) {
            nodes.push(media);
            return nodes;
        }
        match self.describe(content) {
            Shown::Deck(scene) => {
                let group = self.deck_group(&scene, self.full());
                nodes.push(group);
            }
            Shown::Text(slide) => {
                let third = Rect {
                    x: 0.0,
                    y: 0.7,
                    width: 1.0,
                    height: 0.3,
                };
                let area = self.rect_in_safe(third);
                nodes.extend(self.slide_text(&slide, area, true));
            }
            Shown::Nothing => {}
        }
        nodes
    }

    fn background(&mut self, background: &Background, rect: Rect) -> Option<Node> {
        match background {
            Background::None => None,
            Background::Color(c) => Some(self.fill_rect(rect, *c)),
            Background::Image(blob) => Some(self.node(NodeKind::Image(ImageNode {
                blob: *blob,
                rect,
                fit: Fit::Cover,
                crop: None,
            }))),
            Background::Video(blob) => Some(self.node(NodeKind::Video(VideoNode {
                blob: *blob,
                rect,
                fit: Fit::Cover,
                looped: true,
                muted: true,
                autoplay: true,
            }))),
        }
    }

    /// The Logo overlay: the church's logo contained in the safe area on
    /// its background colour, or a ring in its accent colour until it has
    /// one. Without a brand, the ring is in the theme's text colour on
    /// black.
    fn logo_screen(&mut self) -> Vec<Node> {
        let (image, background, accent) = match &self.program.logo {
            Some(logo) => (logo.image, logo.background, logo.accent),
            None => (None, Color::BLACK, self.theme.text.color),
        };
        let mut nodes = Vec::new();
        if background != Color::BLACK {
            nodes.push(self.fill_rect(self.full(), background));
        }
        match image {
            Some(blob) => {
                let inset = Rect {
                    x: self.safe.x + self.safe.width * 0.2,
                    y: self.safe.y + self.safe.height * 0.2,
                    width: self.safe.width * 0.6,
                    height: self.safe.height * 0.6,
                };
                nodes.push(self.node(NodeKind::Image(ImageNode {
                    blob,
                    rect: inset,
                    fit: Fit::Contain,
                    crop: None,
                })));
            }
            None => nodes.push(self.ring(accent)),
        }
        nodes
    }

    /// The idle screen, filling the whole output.
    fn idle(&mut self, idle: &Idle) -> Vec<Node> {
        let full = self.full();
        match idle {
            Idle::Image { blob } => vec![self.node(NodeKind::Image(ImageNode {
                blob: *blob,
                rect: full,
                fit: Fit::Cover,
                crop: None,
            }))],
            Idle::Video { blob } => vec![self.node(NodeKind::Video(VideoNode {
                blob: *blob,
                rect: full,
                fit: Fit::Cover,
                looped: true,
                muted: true,
                autoplay: true,
            }))],
            Idle::Deck { scene } => vec![self.deck_group(scene, full)],
        }
    }

    /// `nodes` with the logo bug on top, when this output shows one.
    fn with_bug(&mut self, mut nodes: Vec<Node>) -> Vec<Node> {
        nodes.extend(self.bug());
        nodes
    }

    /// The logo bug: the logo tight to its corner of the safe area.
    fn bug(&mut self) -> Option<Node> {
        let bug = self.program.logo.as_ref()?.bug.as_ref()?;
        if self.program.overlay != Overlay::None {
            return None;
        }
        // The idle screen is already the logo, large.
        if matches!(&self.program.idle, Some(Idle::Image { blob }) if *blob == bug.image) {
            return None;
        }
        let rect = bug_rect(
            self.safe,
            self.size.height,
            bug.aspect,
            bug.corner,
            bug.size,
        );
        let (blob, opacity) = (bug.image, bug.opacity);
        let mut node = self.node(NodeKind::Image(ImageNode {
            blob,
            rect,
            fit: Fit::Contain,
            crop: None,
        }));
        node.opacity = opacity.clamp(0.0, 1.0);
        Some(node)
    }

    fn ring(&mut self, color: Color) -> Node {
        let r = self.safe.width.min(self.safe.height) * 0.15;
        let c = Point {
            x: self.safe.x + self.safe.width / 2.0,
            y: self.safe.y + self.safe.height / 2.0,
        };
        let k = 0.552_284_8 * r;
        let p = |x: f32, y: f32| Point {
            x: c.x + x,
            y: c.y + y,
        };
        let commands = vec![
            PathCommand::MoveTo(p(r, 0.0)),
            PathCommand::CubicTo(p(r, k), p(k, r), p(0.0, r)),
            PathCommand::CubicTo(p(-k, r), p(-r, k), p(-r, 0.0)),
            PathCommand::CubicTo(p(-r, -k), p(-k, -r), p(0.0, -r)),
            PathCommand::CubicTo(p(k, -r), p(r, -k), p(r, 0.0)),
            PathCommand::Close,
        ];
        let stroke = Stroke {
            paint: Paint::Solid(color),
            width: r * 0.1,
            dash: None,
        };
        self.node(NodeKind::Path(PathNode {
            commands,
            fill: None,
            stroke: Some(stroke),
            shadow: None,
        }))
    }

    /// Body text fitted to `area` (less the footer), then the footer. With
    /// `warn`, records a below-minimum fit.
    fn slide_text(&mut self, slide: &SlideText, area: Rect, warn: bool) -> Vec<Node> {
        let style = self.theme.text.clone();
        let size = style.size * self.scale;
        let min = style.min_size * self.scale;
        let footer = slide.footer.as_deref().filter(|f| !f.is_empty());

        let (footer_rows, footer_size) = match footer {
            Some(f) => {
                let fs = size * FOOTER_SCALE;
                (self.rows(f, &style, fs, area.width), fs)
            }
            None => (0, 0.0),
        };
        let footer_line = footer_size * style.line_spacing;
        let footer_reserved = if footer_rows > 0 {
            (footer_rows as f32 + 0.5) * footer_line
        } else {
            0.0
        };
        let body = Rect {
            height: (area.height - footer_reserved).max(0.0),
            ..area
        };

        let mut out = Vec::new();
        if !slide.lines.is_empty() {
            let fitted = fit_text(
                &slide.lines,
                &style,
                size,
                min,
                body.width,
                body.height,
                self.fonts,
            );
            if warn && fitted.below_minimum {
                self.warnings
                    .push(FitWarning::BelowMinimum { size: fitted.size });
            }
            let block = text_block(body, &slide.lines, &style, fitted.size, style.valign);
            out.push(self.node(NodeKind::Text(block)));
        }
        if let Some(f) = footer {
            let height = footer_rows as f32 * footer_line;
            let rect = Rect {
                x: area.x,
                y: area.y + area.height - height,
                width: area.width,
                height,
            };
            let block = text_block(rect, &[f.to_string()], &style, footer_size, VAlign::Top);
            out.push(self.node(NodeKind::Text(block)));
        }
        out
    }

    /// Rows `text` wraps to at `size` within `width`.
    fn rows(&mut self, text: &str, style: &TextStyle, size: f32, width: f32) -> usize {
        let line = size * style.line_spacing;
        let system = self.fonts.system();
        let mut buffer = Buffer::new(system, Metrics::new(size, line));
        buffer.set_wrap(Wrap::WordOrGlyph);
        buffer.set_size(Some(width), None);
        let attrs = Attrs::new()
            .family(Family::Name(&style.font_family))
            .weight(Weight(style.weight))
            .style(if style.italic {
                Style::Italic
            } else {
                Style::Normal
            });
        let text = if style.uppercase {
            text.to_uppercase()
        } else {
            text.to_string()
        };
        buffer.set_text(&text, &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(system, false);
        buffer.layout_runs().count().max(1)
    }

    fn stage(&mut self) -> Vec<Node> {
        const WHITE: Color = Color::WHITE;
        const DIM: Color = Color::rgb(170, 170, 170);
        const ACCENT: Color = Color::rgb(255, 214, 10);
        let s = self.scale;
        let mut nodes = vec![self.fill_rect(self.full(), Color::BLACK)];
        let stage_style = TextStyle {
            color: WHITE,
            align: HAlign::Left,
            valign: VAlign::Top,
            shadow: None,
            outline: None,
            uppercase: false,
            ..self.theme.text.clone()
        };
        let caption_size = 28.0 * s;
        let caption_h = caption_size * 1.4;

        let fr = |x: f32, y: f32, w: f32, h: f32| {
            self.rect_in_safe(Rect {
                x,
                y,
                width: w,
                height: h,
            })
        };
        let bar = fr(0.0, 0.0, 1.0, 0.08);
        let left = fr(0.0, 0.1, 0.62, 0.9);
        let right = fr(0.66, 0.1, 0.34, 0.9);

        // Top bar: clock left, countdown right.
        let secs = self.now.0.div_euclid(1000) + i64::from(self.utc_offset_minutes) * 60;
        let day = secs.rem_euclid(86_400);
        let clock = format!("{:02}:{:02}", day / 3600, day % 3600 / 60);
        let bar_style = TextStyle {
            weight: 700,
            ..stage_style.clone()
        };
        let clock_rect = Rect {
            width: bar.width / 2.0,
            ..bar
        };
        nodes.push(self.small_text(
            clock_rect,
            &clock,
            &bar_style,
            bar.height * 0.7,
            WHITE,
            HAlign::Left,
        ));
        let countdown = [self.program.content.as_ref(), self.program.next.as_ref()]
            .into_iter()
            .flatten()
            .find_map(|c| match c {
                Content::Countdown { target, .. } => Some(target.0 - self.now.0),
                _ => None,
            });
        if let Some(ms) = countdown {
            let r = Rect {
                x: bar.x + bar.width / 2.0,
                width: bar.width / 2.0,
                ..bar
            };
            let text = format_countdown(ms);
            nodes.push(self.small_text(
                r,
                &text,
                &bar_style,
                bar.height * 0.7,
                ACCENT,
                HAlign::Right,
            ));
        }

        // Current slide.
        let label = self
            .program
            .content
            .as_ref()
            .and_then(slide_label)
            .map(|l| format!("NOW  {l}"))
            .unwrap_or_else(|| "NOW".into());
        let cap = Rect {
            height: caption_h,
            ..left
        };
        nodes.push(self.small_text(cap, &label, &stage_style, caption_size, DIM, HAlign::Left));
        let current_box = Rect {
            y: left.y + caption_h,
            height: left.height - caption_h,
            ..left
        };
        nodes.push(self.outline_rect(current_box, ACCENT, 3.0 * s));
        let inner = inset(current_box, 12.0 * s);
        let current = self.program.content.clone();
        nodes.extend(self.stage_slide(current.as_ref(), inner, &stage_style));

        // Next slide, then notes.
        let next_h = right.height * 0.4;
        let cap = Rect {
            height: caption_h,
            ..right
        };
        nodes.push(self.small_text(cap, "NEXT", &stage_style, caption_size, DIM, HAlign::Left));
        let next_box = Rect {
            y: right.y + caption_h,
            height: next_h - caption_h,
            ..right
        };
        nodes.push(self.outline_rect(next_box, DIM, 2.0 * s));
        let next = self.program.next.clone();
        nodes.extend(self.stage_slide(next.as_ref(), inset(next_box, 8.0 * s), &stage_style));

        let notes_top = right.y + next_h + caption_h * 0.5;
        let cap = Rect {
            y: notes_top,
            height: caption_h,
            ..right
        };
        nodes.push(self.small_text(cap, "NOTES", &stage_style, caption_size, DIM, HAlign::Left));
        let notes_box = Rect {
            y: notes_top + caption_h,
            height: right.y + right.height - notes_top - caption_h,
            ..right
        };
        if let Some(Content::DeckSlide { notes, .. }) = self.program.content.as_ref() {
            if !notes.trim().is_empty() {
                let lines: Vec<String> = notes.lines().map(str::to_string).collect();
                nodes.extend(self.stage_text(&lines, notes_box, &stage_style, 36.0 * s));
            }
        }
        nodes
    }

    fn stage_slide(
        &mut self,
        content: Option<&Content>,
        area: Rect,
        style: &TextStyle,
    ) -> Vec<Node> {
        let Some(content) = content else {
            return Vec::new();
        };
        match self.describe(content) {
            Shown::Text(slide) => {
                let size = self.theme.text.size * self.scale;
                self.stage_text(&slide.lines, area, style, size)
            }
            Shown::Deck(scene) => vec![self.deck_group(&scene, area)],
            Shown::Nothing => {
                let word = if matches!(content, Content::Video { .. }) {
                    "Video"
                } else {
                    "Image"
                };
                self.stage_text(&[word.to_string()], area, style, 48.0 * self.scale)
            }
        }
    }

    /// Text shrunk to a stage box. No minimum is enforced or reported
    /// here: stage boxes are small and only the audience view is flagged.
    fn stage_text(
        &mut self,
        lines: &[String],
        area: Rect,
        style: &TextStyle,
        size: f32,
    ) -> Vec<Node> {
        if lines.is_empty() {
            return Vec::new();
        }
        let min = 8.0 * self.scale;
        let fitted = fit_text(
            lines,
            style,
            size,
            min.min(size),
            area.width,
            area.height,
            self.fonts,
        );
        let block = text_block(area, lines, style, fitted.size, VAlign::Top);
        vec![self.node(NodeKind::Text(block))]
    }

    fn small_text(
        &mut self,
        rect: Rect,
        text: &str,
        style: &TextStyle,
        size: f32,
        color: Color,
        align: HAlign,
    ) -> Node {
        let style = TextStyle {
            color,
            align,
            ..style.clone()
        };
        let block = text_block(rect, &[text.to_string()], &style, size, VAlign::Top);
        self.node(NodeKind::Text(block))
    }

    fn outline_rect(&mut self, rect: Rect, color: Color, width: f32) -> Node {
        self.node(NodeKind::Path(PathNode {
            commands: rect_path(rect),
            fill: None,
            stroke: Some(Stroke {
                paint: Paint::Solid(color),
                width,
                dash: None,
            }),
            shadow: None,
        }))
    }
}

fn slide_label(content: &Content) -> Option<String> {
    match content {
        Content::Lyrics { slide, .. } | Content::Scripture { slide } | Content::Text { slide } => {
            slide.label.clone()
        }
        Content::DeckSlide { index, .. } => Some(format!("Slide {}", index + 1)),
        Content::Countdown { label, .. } => Some(label.clone()).filter(|l| !l.is_empty()),
        _ => None,
    }
}

fn inset(r: Rect, by: f32) -> Rect {
    Rect {
        x: r.x + by,
        y: r.y + by,
        width: (r.width - 2.0 * by).max(0.0),
        height: (r.height - 2.0 * by).max(0.0),
    }
}

fn rect_path(r: Rect) -> Vec<PathCommand> {
    let p = |x, y| Point { x, y };
    vec![
        PathCommand::MoveTo(p(r.x, r.y)),
        PathCommand::LineTo(p(r.x + r.width, r.y)),
        PathCommand::LineTo(p(r.x + r.width, r.y + r.height)),
        PathCommand::LineTo(p(r.x, r.y + r.height)),
        PathCommand::Close,
    ]
}

/// `M:SS`, or `H:MM:SS` from an hour up; zero once the time has passed.
fn format_countdown(remaining_ms: i64) -> String {
    let total = (remaining_ms.max(0) + 999) / 1000;
    let (h, m, s) = (total / 3600, total % 3600 / 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// One paragraph per line, each wrapping on its own, as `fit_text` measures.
fn text_block(
    rect: Rect,
    lines: &[String],
    style: &TextStyle,
    size: f32,
    valign: VAlign,
) -> TextBlock {
    let paragraphs = lines
        .iter()
        .map(|line| Paragraph {
            align: style.align,
            line_spacing: style.line_spacing,
            space_before: 0.0,
            space_after: 0.0,
            indent: 0.0,
            bullet: None,
            runs: vec![TextRun {
                text: if style.uppercase {
                    line.to_uppercase()
                } else {
                    line.clone()
                },
                font_family: style.font_family.clone(),
                size,
                weight: style.weight,
                italic: style.italic,
                underline: false,
                strike: false,
                color: style.color,
            }],
        })
        .collect();
    TextBlock {
        rect,
        valign,
        paragraphs,
        shadow: style.shadow,
        outline: style.outline,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use lampstand_core::deck::DeckId;
    use lampstand_core::id::{BlobHash, Id};
    use lampstand_core::output::OutputTarget;
    use lampstand_core::safe_area::SafeArea;
    use lampstand_core::scene::Transition;
    use lampstand_core::song::SongId;

    const NOW: Timestamp = Timestamp(1_800_000_000_000);

    fn theme(size: f32, min_size: f32) -> Theme {
        Theme {
            id: Id::generate(),
            name: "test".into(),
            text: TextStyle {
                font_family: "Noto Sans".into(),
                size,
                min_size,
                weight: 400,
                italic: false,
                color: Color::WHITE,
                align: HAlign::Center,
                valign: VAlign::Middle,
                line_spacing: 1.2,
                uppercase: false,
                shadow: None,
                outline: None,
            },
            background: Background::Color(Color::rgb(16, 32, 64)),
            text_area: Rect {
                x: 0.0,
                y: 0.1,
                width: 1.0,
                height: 0.8,
            },
            max_lines: 4,
            transition: Transition::CUT,
            show_copyright: true,
        }
    }

    fn output(role: OutputRole, mode: BackgroundMode, safe: SafeArea) -> OutputConfig {
        OutputConfig {
            id: Id::generate(),
            name: "out".into(),
            role,
            target: OutputTarget::Window {
                width: 1920,
                height: 1080,
            },
            safe_area: safe,
            background: mode,
            theme: None,
            idle: None,
            enabled: true,
        }
    }

    fn safe() -> SafeArea {
        SafeArea {
            top: 0.05,
            right: 0.06,
            bottom: 0.08,
            left: 0.04,
        }
    }

    fn slide(label: &str, lines: &[&str], footer: Option<&str>) -> SlideText {
        SlideText {
            label: Some(label.into()),
            lines: lines.iter().map(|s| s.to_string()).collect(),
            footer: footer.map(str::to_string),
        }
    }

    fn deck_scene() -> Arc<Scene> {
        let path = |id, x, color| Node {
            id,
            transform: Transform::IDENTITY,
            opacity: 1.0,
            visible: true,
            kind: NodeKind::Path(PathNode {
                commands: rect_path(Rect {
                    x,
                    y: 100.0,
                    width: 300.0,
                    height: 200.0,
                }),
                fill: Some(Paint::Solid(color)),
                stroke: None,
                shadow: None,
            }),
        };
        Arc::new(Scene {
            width: 960.0,
            height: 540.0,
            nodes: vec![
                path(1, 50.0, Color::rgb(200, 30, 30)),
                path(2, 500.0, Color::rgb(30, 30, 200)),
            ],
        })
    }

    fn lyrics() -> Content {
        Content::Lyrics {
            song: SongId::generate(),
            slide: slide(
                "V1",
                &[
                    "Amazing grace how sweet the sound",
                    "That saved a wretch like me",
                    "I once was lost but now am found",
                    "Was blind but now I see",
                ],
                Some("Public domain"),
            ),
        }
    }

    fn scripture() -> Content {
        Content::Scripture {
            slide: slide(
                "John 3:16–17",
                &[
                    "16 For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life.",
                    "17 For God sent not his Son into the world to condemn the world; but that the world through him might be saved.",
                ],
                Some("John 3:16–17 (KJV)"),
            ),
        }
    }

    fn deck() -> Content {
        Content::DeckSlide {
            deck: DeckId::generate(),
            index: 2,
            step: 0,
            scene: deck_scene(),
            notes: "Welcome everyone.\nMention the picnic.".into(),
        }
    }

    fn program(content: Option<Content>, overlay: Overlay) -> Program {
        Program {
            background: None,
            content,
            overlay,
            next: Some(Content::Text {
                slide: slide("Next", &["Announcements"], None),
            }),
            theme: None,
            idle: None,
            logo: None,
        }
    }

    fn run(p: &Program, o: &OutputConfig, w: f32, h: f32, t: &Theme) -> Composed {
        let mut fonts = Fonts::bundled_only();
        compose_at(
            p,
            o,
            OutputSize {
                width: w,
                height: h,
            },
            t,
            &mut fonts,
            NOW,
            0,
        )
    }

    fn walk<'a>(nodes: &'a [Node], f: &mut impl FnMut(&'a Node)) {
        for n in nodes {
            f(n);
            if let NodeKind::Group(children) = &n.kind {
                walk(children, f);
            }
        }
    }

    fn roles() -> [(&'static str, OutputRole); 3] {
        [
            ("audience", OutputRole::Audience { primary: true }),
            ("stage", OutputRole::Stage),
            ("stream", OutputRole::Stream),
        ]
    }

    fn contents() -> Vec<(&'static str, Option<Content>, Overlay)> {
        vec![
            ("lyrics", Some(lyrics()), Overlay::None),
            ("scripture", Some(scripture()), Overlay::None),
            ("deck", Some(deck()), Overlay::None),
            ("black", Some(lyrics()), Overlay::Black),
            ("logo", Some(lyrics()), Overlay::Logo),
            ("nothing", None, Overlay::None),
        ]
    }

    const SHAPES: [(&str, f32, f32); 3] = [
        ("4x3", 1440.0, 1080.0),
        ("16x9", 1920.0, 1080.0),
        ("32x9", 3840.0, 1080.0),
    ];

    #[test]
    fn snapshots_for_every_role_shape_and_content() {
        let t = theme(60.0, 30.0);
        for (role_name, role) in roles() {
            let o = output(role, BackgroundMode::Bleed, safe());
            for (shape, w, h) in SHAPES {
                for (name, content, overlay) in contents() {
                    let composed = run(&program(content, overlay), &o, w, h, &t);
                    let json = serde_json::to_value(&composed.scene).unwrap();
                    insta::assert_json_snapshot!(format!("{role_name}_{shape}_{name}"), json);
                }
            }
        }
    }

    #[test]
    fn text_always_sits_inside_the_safe_area() {
        let safes = [
            SafeArea::default(),
            safe(),
            SafeArea {
                top: 0.2,
                right: 0.15,
                bottom: 0.1,
                left: 0.3,
            },
        ];
        let t = theme(60.0, 30.0);
        for s in safes {
            for (_, role) in roles() {
                let o = output(role, BackgroundMode::Bleed, s);
                for (_, w, h) in SHAPES {
                    let area = s.rect(w, h);
                    for (name, content, overlay) in contents() {
                        let c = run(&program(content, overlay), &o, w, h, &t);
                        walk(&c.scene.nodes, &mut |n| {
                            if let NodeKind::Text(b) = &n.kind {
                                let r = b.rect;
                                let e = 0.01;
                                assert!(
                                    r.x >= area.x - e
                                        && r.y >= area.y - e
                                        && r.x + r.width <= area.x + area.width + e
                                        && r.y + r.height <= area.y + area.height + e,
                                    "{name} {r:?} outside {area:?}"
                                );
                            }
                        });
                    }
                }
            }
        }
    }

    fn first_fill(scene: &Scene, index: usize) -> Rect {
        match &scene.nodes[index].kind {
            NodeKind::Path(p) => {
                let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
                for c in &p.commands {
                    if let PathCommand::MoveTo(q) | PathCommand::LineTo(q) = c {
                        x0 = x0.min(q.x);
                        y0 = y0.min(q.y);
                        x1 = x1.max(q.x);
                        y1 = y1.max(q.y);
                    }
                }
                Rect {
                    x: x0,
                    y: y0,
                    width: x1 - x0,
                    height: y1 - y0,
                }
            }
            other => panic!("not a path: {other:?}"),
        }
    }

    #[test]
    fn backgrounds_bleed_or_fit_as_configured() {
        let t = theme(60.0, 30.0);
        let p = program(Some(lyrics()), Overlay::None);
        let role = OutputRole::Audience { primary: true };

        let bleed = run(
            &p,
            &output(role, BackgroundMode::Bleed, safe()),
            1920.0,
            1080.0,
            &t,
        );
        let full = Rect {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };
        assert_eq!(first_fill(&bleed.scene, 1), full);

        let fit = run(
            &p,
            &output(role, BackgroundMode::Fit, safe()),
            1920.0,
            1080.0,
            &t,
        );
        assert_eq!(first_fill(&fit.scene, 1), safe().rect(1920.0, 1080.0));
        // Black still fills the screen behind a fitted background.
        assert_eq!(first_fill(&fit.scene, 0), full);
    }

    #[test]
    fn program_background_overrides_theme_and_none_draws_nothing() {
        let t = theme(60.0, 30.0);
        let role = OutputRole::Audience { primary: true };
        let o = output(role, BackgroundMode::Bleed, safe());
        let mut p = program(None, Overlay::None);
        p.background = Some(Background::Image(BlobHash::of(b"bg")));
        let c = run(&p, &o, 1920.0, 1080.0, &t);
        assert!(matches!(&c.scene.nodes[1].kind, NodeKind::Image(i) if i.fit == Fit::Cover));
        p.background = Some(Background::None);
        let c = run(&p, &o, 1920.0, 1080.0, &t);
        assert_eq!(c.scene.nodes.len(), 2 - 1, "black base only");
    }

    #[test]
    fn stream_is_transparent_with_a_lower_third() {
        let t = theme(60.0, 30.0);
        let o = output(OutputRole::Stream, BackgroundMode::Bleed, safe());
        let c = run(
            &program(Some(lyrics()), Overlay::None),
            &o,
            1920.0,
            1080.0,
            &t,
        );
        let area = safe().rect(1920.0, 1080.0);
        assert!(c
            .scene
            .nodes
            .iter()
            .all(|n| matches!(n.kind, NodeKind::Text(_))));
        walk(&c.scene.nodes, &mut |n| {
            if let NodeKind::Text(b) = &n.kind {
                assert!(b.rect.y >= area.y + area.height * 0.7 - 0.01);
            }
        });
        let empty = run(
            &program(Some(lyrics()), Overlay::Black),
            &o,
            1920.0,
            1080.0,
            &t,
        );
        assert!(empty.scene.nodes.is_empty());
    }

    #[test]
    fn deck_is_fitted_to_the_safe_area_keeping_aspect_and_ids() {
        let t = theme(60.0, 30.0);
        let o = output(
            OutputRole::Audience { primary: true },
            BackgroundMode::Bleed,
            safe(),
        );
        let (w, h) = (1440.0, 1080.0);
        let c = run(&program(Some(deck()), Overlay::None), &o, w, h, &t);
        let group = c.scene.nodes.last().unwrap();
        let NodeKind::Group(children) = &group.kind else {
            panic!("expected a group")
        };
        let ids: Vec<_> = children.iter().map(|n| n.id).collect();
        assert_eq!(ids, [1, 2]);
        assert!(group.id >= COMPOSITOR_NODE_BASE);

        let expected = safe().fit(w, h, 960.0 / 540.0);
        let tl = group.transform.apply(Point { x: 0.0, y: 0.0 });
        let br = group.transform.apply(Point { x: 960.0, y: 540.0 });
        let near = |a: f32, b: f32| (a - b).abs() < 0.01;
        assert!(near(tl.x, expected.x) && near(tl.y, expected.y));
        assert!(near(br.x - tl.x, expected.width) && near(br.y - tl.y, expected.height));
        assert!(near(group.transform.a, group.transform.d), "aspect kept");
    }

    #[test]
    fn compositor_nodes_are_numbered_from_the_base_and_unique() {
        let t = theme(60.0, 30.0);
        for (_, role) in roles() {
            let o = output(role, BackgroundMode::Bleed, safe());
            let c = run(
                &program(Some(lyrics()), Overlay::None),
                &o,
                1920.0,
                1080.0,
                &t,
            );
            let mut seen = std::collections::HashSet::new();
            walk(&c.scene.nodes, &mut |n| {
                assert!(n.id >= COMPOSITOR_NODE_BASE);
                assert!(seen.insert(n.id), "duplicate id {}", n.id);
            });
        }
    }

    #[test]
    fn overlays_cover_the_audience_output() {
        let t = theme(60.0, 30.0);
        let o = output(
            OutputRole::Audience { primary: true },
            BackgroundMode::Fit,
            safe(),
        );
        let black = run(
            &program(Some(lyrics()), Overlay::Black),
            &o,
            1920.0,
            1080.0,
            &t,
        );
        assert_eq!(black.scene.nodes.len(), 1);
        assert_eq!(first_fill(&black.scene, 0).width, 1920.0);
        let logo = run(
            &program(Some(lyrics()), Overlay::Logo),
            &o,
            1920.0,
            1080.0,
            &t,
        );
        assert_eq!(logo.scene.nodes.len(), 2);
    }

    fn blob(n: u8) -> lampstand_core::id::BlobHash {
        lampstand_core::id::BlobHash::of(&[n])
    }

    fn kinds(scene: &Scene) -> Vec<&'static str> {
        scene
            .nodes
            .iter()
            .map(|n| match n.kind {
                NodeKind::Image(_) => "image",
                NodeKind::Video(_) => "video",
                NodeKind::Group(_) => "group",
                NodeKind::Path(_) => "path",
                NodeKind::Text(_) => "text",
            })
            .collect()
    }

    #[test]
    fn the_logo_screen_shows_the_church_logo_on_its_colour() {
        use lampstand_core::program::Logo;
        let t = theme(60.0, 30.0);
        let o = output(
            OutputRole::Audience { primary: true },
            BackgroundMode::Bleed,
            safe(),
        );
        let mut p = program(None, Overlay::Logo);
        p.logo = Some(Logo {
            image: Some(blob(1)),
            background: Color::rgb(20, 30, 90),
            accent: Color::WHITE,
            bug: None,
        });
        let c = run(&p, &o, 1920.0, 1080.0, &t);
        assert_eq!(kinds(&c.scene), ["path", "path", "image"]);
        // No logo chosen yet: a ring in the accent colour.
        p.logo.as_mut().unwrap().image = None;
        let c = run(&p, &o, 1920.0, 1080.0, &t);
        assert_eq!(kinds(&c.scene), ["path", "path", "path"]);
    }

    fn with_bug(mut p: Program, corner: lampstand_core::brand::BugCorner) -> Program {
        use lampstand_core::brand::BugSize;
        use lampstand_core::program::{Bug, Logo};
        p.logo = Some(Logo {
            image: Some(blob(1)),
            background: Color::rgb(20, 30, 90),
            accent: Color::WHITE,
            bug: Some(Bug {
                image: blob(1),
                aspect: 2.0,
                corner,
                size: BugSize::Medium,
                opacity: 0.6,
            }),
        });
        p
    }

    fn bug_of(c: &Composed) -> Option<&Node> {
        c.scene
            .nodes
            .last()
            .filter(|n| matches!(&n.kind, NodeKind::Image(i) if i.blob == blob(1)))
    }

    #[test]
    fn the_bug_is_drawn_last_in_the_safe_area_corner_of_audience_and_stream() {
        use lampstand_core::brand::BugCorner;
        let t = theme(60.0, 30.0);
        for (role, mode) in [
            (
                OutputRole::Audience { primary: true },
                BackgroundMode::Bleed,
            ),
            (OutputRole::Audience { primary: true }, BackgroundMode::Fit),
            (OutputRole::Stream, BackgroundMode::Bleed),
        ] {
            let o = output(role, mode, safe());
            for (w, h) in [(1920.0, 1080.0), (1280.0, 1024.0), (1024.0, 768.0)] {
                let safe_rect = o.safe_area.rect(w, h);
                for corner in [BugCorner::LowerLeft, BugCorner::LowerRight] {
                    let p = with_bug(program(Some(lyrics()), Overlay::None), corner);
                    let c = run(&p, &o, w, h, &t);
                    let node = bug_of(&c).expect("the bug is on top");
                    let NodeKind::Image(img) = &node.kind else {
                        unreachable!()
                    };
                    assert!((node.opacity - 0.6).abs() < 1e-6);
                    assert!(img.rect.x >= safe_rect.x && img.rect.y >= safe_rect.y);
                    assert!(img.rect.x + img.rect.width <= safe_rect.x + safe_rect.width + 0.01);
                    assert!(img.rect.y + img.rect.height <= safe_rect.y + safe_rect.height + 0.01);
                    let centre = img.rect.x + img.rect.width / 2.0;
                    let lower_right = centre > safe_rect.x + safe_rect.width / 2.0;
                    assert_eq!(lower_right, corner == BugCorner::LowerRight);
                    assert!(img.rect.y > safe_rect.y + safe_rect.height / 2.0);
                }
            }
        }
    }

    #[test]
    fn the_bug_stays_over_media_deck_slides_and_an_empty_screen() {
        use lampstand_core::brand::BugCorner;
        let t = theme(60.0, 30.0);
        let o = output(
            OutputRole::Audience { primary: true },
            BackgroundMode::Bleed,
            safe(),
        );
        for content in [
            None,
            Some(Content::Image { blob: blob(5) }),
            Some(deck()),
            Some(lyrics()),
        ] {
            let p = with_bug(program(content, Overlay::None), BugCorner::LowerRight);
            assert!(bug_of(&run(&p, &o, 1920.0, 1080.0, &t)).is_some());
        }
        // The same slide on the next frame: only the slide changes.
        let a = with_bug(
            program(Some(lyrics()), Overlay::None),
            BugCorner::LowerRight,
        );
        let b = with_bug(program(None, Overlay::None), BugCorner::LowerRight);
        assert_eq!(
            bug_of(&run(&a, &o, 1920.0, 1080.0, &t)).map(|n| n.kind.clone()),
            bug_of(&run(&b, &o, 1920.0, 1080.0, &t)).map(|n| n.kind.clone())
        );
    }

    #[test]
    fn the_bug_is_hidden_under_black_and_logo_and_on_the_stage() {
        use lampstand_core::brand::BugCorner;
        let t = theme(60.0, 30.0);
        let audience = output(
            OutputRole::Audience { primary: true },
            BackgroundMode::Bleed,
            safe(),
        );
        for overlay in [Overlay::Black, Overlay::Logo] {
            let p = with_bug(program(Some(lyrics()), overlay), BugCorner::LowerRight);
            let c = run(&p, &audience, 1920.0, 1080.0, &t);
            // Logo's own full-screen picture is not the bug.
            assert!(c.scene.nodes.iter().all(|n| match &n.kind {
                NodeKind::Image(i) => i.rect.width > 1000.0,
                _ => true,
            }));
            let stream = output(OutputRole::Stream, BackgroundMode::Bleed, safe());
            assert!(run(&p, &stream, 1920.0, 1080.0, &t).scene.nodes.is_empty());
        }
        let stage = output(OutputRole::Stage, BackgroundMode::Bleed, safe());
        let p = with_bug(
            program(Some(lyrics()), Overlay::None),
            BugCorner::LowerRight,
        );
        assert!(bug_of(&run(&p, &stage, 1920.0, 1080.0, &t)).is_none());
    }

    #[test]
    fn the_bug_is_hidden_when_the_idle_screen_is_the_logo_but_not_other_idle_pictures() {
        use lampstand_core::brand::BugCorner;
        let t = theme(60.0, 30.0);
        let o = output(
            OutputRole::Audience { primary: true },
            BackgroundMode::Bleed,
            safe(),
        );
        let mut p = with_bug(program(None, Overlay::None), BugCorner::LowerRight);
        p.idle = Some(Idle::Image { blob: blob(1) });
        let c = run(&p, &o, 1920.0, 1080.0, &t);
        assert_eq!(kinds(&c.scene), ["path", "image"]);
        p.idle = Some(Idle::Image { blob: blob(2) });
        let c = run(&p, &o, 1920.0, 1080.0, &t);
        assert_eq!(kinds(&c.scene), ["path", "image", "image"]);
    }

    #[test]
    fn idle_fills_an_audience_screen_and_video_loops_muted() {
        let t = theme(60.0, 30.0);
        let audience = output(
            OutputRole::Audience { primary: true },
            BackgroundMode::Fit,
            safe(),
        );
        let mut p = program(None, Overlay::None);
        p.idle = Some(Idle::Image { blob: blob(2) });
        let c = run(&p, &audience, 1920.0, 1080.0, &t);
        assert_eq!(kinds(&c.scene), ["path", "image"]);
        p.idle = Some(Idle::Video { blob: blob(3) });
        let c = run(&p, &audience, 1920.0, 1080.0, &t);
        let NodeKind::Video(v) = &c.scene.nodes[1].kind else {
            panic!("expected a video");
        };
        assert!(v.looped && v.muted && v.autoplay);
        assert_eq!(v.rect.width, 1920.0, "idle ignores the safe area");
        p.idle = Some(Idle::Deck {
            scene: deck_scene(),
        });
        let c = run(&p, &audience, 1920.0, 1080.0, &t);
        assert_eq!(kinds(&c.scene), ["path", "group"]);
    }

    #[test]
    fn black_and_logo_still_cover_the_idle_screen() {
        let t = theme(60.0, 30.0);
        let o = output(
            OutputRole::Audience { primary: true },
            BackgroundMode::Bleed,
            safe(),
        );
        let mut p = program(None, Overlay::Black);
        p.idle = Some(Idle::Image { blob: blob(2) });
        assert_eq!(kinds(&run(&p, &o, 1920.0, 1080.0, &t).scene), ["path"]);
        p.overlay = Overlay::Logo;
        assert_eq!(
            kinds(&run(&p, &o, 1920.0, 1080.0, &t).scene),
            ["path", "path"]
        );
    }

    #[test]
    fn stage_and_stream_ignore_idle() {
        let t = theme(60.0, 30.0);
        let mut p = program(None, Overlay::None);
        p.idle = Some(Idle::Image { blob: blob(2) });
        for role in [OutputRole::Stage, OutputRole::Stream] {
            let o = output(role, BackgroundMode::Bleed, safe());
            let c = run(&p, &o, 1920.0, 1080.0, &t);
            assert!(!kinds(&c.scene).contains(&"image"), "{role:?}");
        }
    }

    #[test]
    fn text_scales_with_output_height() {
        let t = theme(60.0, 30.0);
        let o = output(
            OutputRole::Audience { primary: true },
            BackgroundMode::Bleed,
            SafeArea::default(),
        );
        let p = program(
            Some(Content::Text {
                slide: slide("a", &["Hello"], None),
            }),
            Overlay::None,
        );
        let size_of = |h: f32| {
            let c = run(&p, &o, h * 16.0 / 9.0, h, &t);
            let mut size = 0.0;
            walk(&c.scene.nodes, &mut |n| {
                if let NodeKind::Text(b) = &n.kind {
                    size = b.paragraphs[0].runs[0].size;
                }
            });
            size
        };
        assert_eq!(size_of(1080.0), 60.0);
        assert_eq!(size_of(2160.0), 120.0);
        assert_eq!(size_of(540.0), 30.0);
    }

    #[test]
    fn footer_is_half_size_below_the_body() {
        let t = theme(60.0, 30.0);
        let o = output(
            OutputRole::Audience { primary: true },
            BackgroundMode::Bleed,
            SafeArea::default(),
        );
        let c = run(
            &program(Some(scripture()), Overlay::None),
            &o,
            1920.0,
            1080.0,
            &t,
        );
        let blocks: Vec<&TextBlock> = c
            .scene
            .nodes
            .iter()
            .filter_map(|n| match &n.kind {
                NodeKind::Text(b) => Some(b),
                _ => None,
            })
            .collect();
        assert_eq!(blocks.len(), 2);
        let (body, footer) = (blocks[0], blocks[1]);
        assert_eq!(footer.paragraphs[0].runs[0].size, 30.0);
        assert!(body.rect.y + body.rect.height < footer.rect.y);
        let area = t.text_area;
        assert!(
            (footer.rect.y + footer.rect.height - 1080.0 * (area.y + area.height)).abs() < 0.01
        );
    }

    #[test]
    fn warns_when_text_needs_less_than_the_minimum() {
        let o = output(
            OutputRole::Audience { primary: true },
            BackgroundMode::Bleed,
            safe(),
        );
        let long = Content::Text {
            slide: slide("a", &[&"word ".repeat(400)], None),
        };
        let p = program(Some(long), Overlay::None);
        let c = run(&p, &o, 1920.0, 1080.0, &theme(60.0, 40.0));
        assert_eq!(c.warnings, vec![FitWarning::BelowMinimum { size: 40.0 }]);
        // The minimum scales with the output.
        let c = run(&p, &o, 960.0, 540.0, &theme(60.0, 40.0));
        assert_eq!(c.warnings, vec![FitWarning::BelowMinimum { size: 20.0 }]);
        // Short text fits and does not warn.
        let ok = run(
            &program(Some(lyrics()), Overlay::None),
            &o,
            1920.0,
            1080.0,
            &theme(60.0, 30.0),
        );
        assert!(ok.warnings.is_empty());
    }

    #[test]
    fn stage_clock_follows_the_utc_offset() {
        let t = theme(60.0, 30.0);
        let o = output(OutputRole::Stage, BackgroundMode::Bleed, safe());
        let p = program(Some(lyrics()), Overlay::None);
        let clock = |offset: i32| {
            let mut fonts = Fonts::bundled_only();
            let c = crate::compose_at(
                &p,
                &o,
                OutputSize {
                    width: 1920.0,
                    height: 1080.0,
                },
                &t,
                &mut fonts,
                NOW,
                offset,
            );
            let mut texts = Vec::new();
            walk(&c.scene.nodes, &mut |n| {
                if let NodeKind::Text(b) = &n.kind {
                    texts.push(b.paragraphs[0].runs[0].text.clone());
                }
            });
            texts
        };
        // 1_800_000_000 s is 08:00:00 UTC.
        assert!(clock(0).contains(&"08:00".to_string()));
        let jamaica = clock(-300);
        assert!(jamaica.contains(&"03:00".to_string()), "{jamaica:?}");
        assert!(!jamaica.contains(&"08:00".to_string()), "{jamaica:?}");
    }

    #[test]
    fn countdown_and_clock_are_formatted() {
        assert_eq!(format_countdown(65_000), "1:05");
        assert_eq!(format_countdown(3_725_000), "1:02:05");
        assert_eq!(format_countdown(-5), "0:00");
        assert_eq!(format_countdown(1), "0:01");

        let t = theme(60.0, 30.0);
        let o = output(OutputRole::Stage, BackgroundMode::Bleed, safe());
        let p = program(
            Some(Content::Countdown {
                target: Timestamp(NOW.0 + 90_000),
                label: "Service starts".into(),
            }),
            Overlay::None,
        );
        let mut fonts = Fonts::bundled_only();
        let c = compose_at(
            &p,
            &o,
            OutputSize {
                width: 1920.0,
                height: 1080.0,
            },
            &t,
            &mut fonts,
            NOW,
            60,
        );
        let mut texts = Vec::new();
        walk(&c.scene.nodes, &mut |n| {
            if let NodeKind::Text(b) = &n.kind {
                texts.push(b.paragraphs[0].runs[0].text.clone());
            }
        });
        // 1_800_000_000 s is 08:00:00 UTC; plus a 60-minute offset.
        assert!(texts.contains(&"09:00".to_string()), "{texts:?}");
        assert!(
            texts.iter().filter(|t| *t == "1:30").count() >= 2,
            "{texts:?}"
        );
    }
}
