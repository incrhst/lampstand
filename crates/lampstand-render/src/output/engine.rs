//! The render thread's frame loop.
//!
//! State is split by what survives a panic:
//!
//! - `Logical` (per output, kept): what the output should show — the scene,
//!   the transition and build steps in progress, overlays. Commands for an
//!   output that has no surface yet land here too, so attaching a window
//!   shows the current scene in its first frame.
//! - `Attached` (per output, kept): the surface and its configuration.
//! - `Work` (per output, rebuilt after a panic): the textures each scene on
//!   screen has been drawn into.
//!
//! Each loop iteration drains the command channel without blocking, draws
//! every output that needs a frame, and presents it. Only when no output is
//! changing does it block, on the channel, so a static slide costs no GPU
//! time. Presenting is FIFO; with several outputs, acquiring each surface's
//! next texture paces the loop to the displays.

use std::collections::{HashMap, HashSet};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use lampstand_core::id::BlobHash;
use lampstand_core::output::OutputId;
use lampstand_core::safe_area::SafeArea;
use lampstand_core::scene::{Effect, Scene, Transition, TransitionKind};

use super::composite::Compositor;
use super::overlay;
use super::timeline::{progress, transition_layers, AnimationTimeline, CompositeLayer, Which};
use super::video::{FrameView, PlayerKey, Players, VideoClock, VideoKey};
use super::{Background, FrameTiming, OutputEvent, OutputOptions};
use crate::gpu::{Texture, FORMAT};
use crate::{CalibrationPattern, Image, OutputCommand, RenderError, Renderer, VideoFrames};

/// How long the loop sleeps when nothing is changing. Commands wake it at
/// once; this only bounds housekeeping.
const IDLE: Duration = Duration::from_millis(500);
/// Retry delay after a surface reports a timeout or occlusion.
const RETRY: Duration = Duration::from_millis(16);
/// Headless outputs have no vsync to pace them. Without this, an animating
/// headless output re-renders as fast as the loop spins, piling GPU work up
/// faster than a software adapter (WARP, lavapipe) finishes it, and a
/// snapshot then waits behind the whole queue.
const HEADLESS_FRAME: Duration = Duration::from_millis(16);
/// If a hidden window's surface keeps timing out, show it anyway after
/// this, so the output is never stuck invisible.
const FIRST_FRAME_FALLBACK: Duration = Duration::from_millis(500);
/// Two panics on one output within this window blank it instead of
/// restarting with a scene that keeps crashing.
const PANIC_WINDOW: Duration = Duration::from_secs(10);
/// Decoded images kept before those no output shows are dropped.
const IMAGE_BUDGET: usize = 48;

pub(crate) enum Msg {
    Command {
        output: OutputId,
        command: OutputCommand,
        sent: Instant,
    },
    Attach {
        output: OutputId,
        surface: Option<wgpu::Surface<'static>>,
        width: u32,
        height: u32,
        background: Background,
    },
    Resize {
        output: OutputId,
        width: u32,
        height: u32,
    },
    Detach {
        output: OutputId,
        done: Sender<()>,
    },
    Snapshot {
        output: OutputId,
        reply: Sender<Result<Image, RenderError>>,
    },
    InjectPanic(Option<OutputId>),
}

#[derive(Default)]
struct Logical {
    scene: Option<Arc<Scene>>,
    /// Bumped by every change to the scene or its animations.
    rev: u64,
    transition: Option<(Transition, f64)>,
    timeline: AnimationTimeline,
    safe_area: Option<SafeArea>,
    calibrate: Option<CalibrationPattern>,
    /// When the oldest command not yet on screen was sent.
    pending: Option<Instant>,
    dirty: bool,
    videos: VideoClock,
    /// A video on this output has a new frame, so the scene needs drawing
    /// again.
    video_frame: bool,
    panics: Vec<Instant>,
}

enum Sink {
    Surface {
        surface: wgpu::Surface<'static>,
        config: wgpu::SurfaceConfiguration,
        view_format: wgpu::TextureFormat,
    },
    Headless {
        texture: Texture,
    },
}

#[derive(Default)]
struct Work {
    incoming: Option<Texture>,
    incoming_rev: u64,
    /// The incoming texture was drawn mid-animation, so it needs redrawing
    /// even if nothing else changed.
    incoming_live: bool,
    outgoing: Option<Texture>,
    spare: Option<Texture>,
    overlay: Option<(SafeArea, Texture)>,
    calibration: Option<(CalibrationPattern, Texture)>,
}

struct Attached {
    sink: Sink,
    size: (u32, u32),
    background: Background,
    /// `OutputEvent::FirstFrame` has been sent.
    shown: bool,
    attached_at: Instant,
    retry_at: Option<Instant>,
    lost: bool,
    frame: u64,
    panic_next: bool,
    work: Work,
}

pub(crate) struct Engine {
    pub renderer: Renderer,
    pub compositor: Compositor,
    pub options: OutputOptions,
    pub events: Box<dyn Fn(OutputEvent) + Send>,
    pub clock: Instant,
    logical: HashMap<OutputId, Logical>,
    attached: HashMap<OutputId, Attached>,
    players: Players,
    thread_panics: Vec<Instant>,
}

fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = panic.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = panic.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".into()
    }
}

/// The render thread's body: runs the loop, and after a panic rebuilds the
/// GPU working state and carries on with the last scenes.
pub(crate) fn run(mut engine: Engine, rx: Receiver<Msg>) {
    loop {
        match catch_unwind(AssertUnwindSafe(|| engine.frame_loop(&rx))) {
            Ok(()) => return,
            Err(panic) => {
                let message = panic_message(&*panic);
                engine.recover(None, message);
            }
        }
    }
}

impl Engine {
    pub fn new(
        renderer: Renderer,
        options: OutputOptions,
        events: Box<dyn Fn(OutputEvent) + Send>,
    ) -> Self {
        let compositor = Compositor::new(&renderer.gpu, &renderer.pipelines);
        Self {
            renderer,
            compositor,
            options,
            events,
            clock: Instant::now(),
            logical: HashMap::new(),
            attached: HashMap::new(),
            players: Players::default(),
            thread_panics: Vec::new(),
        }
    }

    fn now_ms(&self) -> f64 {
        self.clock.elapsed().as_secs_f64() * 1000.0
    }

    fn frame_loop(&mut self, rx: &Receiver<Msg>) {
        loop {
            let wait = self.next_wake(Instant::now());
            if !wait.is_zero() {
                match rx.recv_timeout(wait) {
                    Ok(msg) => self.handle(msg),
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
            loop {
                match rx.try_recv() {
                    Ok(msg) => self.handle(msg),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return,
                }
            }
            self.render_all();
            // Runs completed-work callbacks and frees finished buffers.
            let _ = self.renderer.gpu.device.poll(wgpu::PollType::Poll);
        }
    }

    fn needs_frame(&self, id: &OutputId, now_ms: f64) -> bool {
        let Some(a) = self.attached.get(id) else {
            return false;
        };
        if a.lost {
            return false;
        }
        let Some(l) = self.logical.get(id) else {
            return !a.shown;
        };
        l.dirty
            || !a.shown
            || a.work.incoming_live
            || l.video_frame
            || l.transition.is_some()
            || l.timeline.running(now_ms)
            || l.videos
                .players()
                .any(|(_, key)| self.players.running(&key))
    }

    /// How long the loop may block before some output needs a frame.
    fn next_wake(&self, now: Instant) -> Duration {
        let now_ms = self.now_ms();
        let mut wait = IDLE;
        for (id, a) in &self.attached {
            if !self.needs_frame(id, now_ms) {
                continue;
            }
            match a.retry_at {
                Some(at) if at > now => wait = wait.min(at - now),
                _ => return Duration::ZERO,
            }
        }
        wait
    }

    fn emit(&self, event: OutputEvent) {
        (self.events)(event);
    }

    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Command {
                output,
                command,
                sent,
            } => self.command(output, command, sent),
            Msg::Attach {
                output,
                surface,
                width,
                height,
                background,
            } => {
                let size = (width.max(1), height.max(1));
                let sink = match surface {
                    Some(surface) => match self.configure(surface, size, background) {
                        Ok(sink) => sink,
                        Err(e) => {
                            self.emit(OutputEvent::SurfaceLost {
                                output,
                                reason: format!("{e:?}"),
                            });
                            return;
                        }
                    },
                    None => Sink::Headless {
                        texture: Texture::new(
                            &self.renderer.gpu.device,
                            "headless output",
                            size.0,
                            size.1,
                            1,
                            1,
                        ),
                    },
                };
                self.attached.insert(
                    output,
                    Attached {
                        sink,
                        size,
                        background,
                        shown: false,
                        attached_at: Instant::now(),
                        retry_at: None,
                        lost: false,
                        frame: 0,
                        panic_next: false,
                        work: Work::default(),
                    },
                );
                self.logical.entry(output).or_default().dirty = true;
            }
            Msg::Resize {
                output,
                width,
                height,
            } => {
                let size = (width.max(1), height.max(1));
                let device = &self.renderer.gpu.device;
                if let Some(a) = self.attached.get_mut(&output) {
                    if a.size == size {
                        return;
                    }
                    a.size = size;
                    match &mut a.sink {
                        Sink::Surface {
                            surface, config, ..
                        } => {
                            config.width = size.0;
                            config.height = size.1;
                            surface.configure(device, config);
                        }
                        Sink::Headless { texture } => {
                            *texture =
                                Texture::new(device, "headless output", size.0, size.1, 1, 1);
                        }
                    }
                    a.work = Work::default();
                    a.lost = false;
                }
                let l = self.logical.entry(output).or_default();
                l.transition = None;
                l.dirty = true;
            }
            Msg::Detach { output, done } => {
                // Dropping the surface here, on the thread that presents to
                // it, before the caller destroys the window.
                self.attached.remove(&output);
                let _ = done.send(());
            }
            Msg::Snapshot { output, reply } => {
                let _ = reply.send(self.snapshot(output));
            }
            Msg::InjectPanic(Some(output)) => {
                if let Some(a) = self.attached.get_mut(&output) {
                    a.panic_next = true;
                }
            }
            Msg::InjectPanic(None) => panic!("injected render-thread panic"),
        }
    }

    fn command(&mut self, output: OutputId, command: OutputCommand, sent: Instant) {
        let now_ms = self.now_ms();
        if matches!(&command, OutputCommand::Show { transition, .. }
            if transition.kind != TransitionKind::Cut && transition.duration_ms > 0)
        {
            // The outgoing scene must be drawn before it can transition out,
            // even if it arrived in the same batch and never got a frame.
            self.draw_pending(output, now_ms);
        }
        let is_show = matches!(command, OutputCommand::Show { .. });
        // Files other outputs are playing, so a video another output
        // already shows joins it instead of starting over.
        let mut playing = HashMap::new();
        for (id, other) in &self.logical {
            if *id != output {
                for ((_, blob), &start) in &other.videos.started {
                    playing.insert(*blob, start);
                }
            }
        }
        let l = self.logical.entry(output).or_default();
        let attached = self.attached.get_mut(&output);
        match command {
            OutputCommand::Show { scene, transition } => {
                let changed = l
                    .scene
                    .as_ref()
                    .is_none_or(|old| !Arc::ptr_eq(old, &scene) && **old != *scene);
                let animated = changed
                    && l.scene.is_some()
                    && transition.kind != TransitionKind::Cut
                    && transition.duration_ms > 0;
                l.videos.show_joining(&scene, now_ms, &playing);
                l.scene = Some(scene);
                l.timeline = AnimationTimeline::default();
                l.rev += 1;
                if animated {
                    // A transition still running is cut short: its incoming
                    // scene, as last drawn, becomes the new outgoing one.
                    l.transition = Some((transition, now_ms));
                    if let Some(a) = attached {
                        let old = a.work.outgoing.take();
                        a.work.outgoing = a.work.incoming.take();
                        a.work.spare = old.or(a.work.spare.take());
                    }
                } else if l.transition.take().is_some() {
                    if let Some(a) = attached {
                        a.work.spare = a.work.outgoing.take();
                    }
                }
            }
            OutputCommand::Animate(animations) => {
                if let Some(scene) = &l.scene {
                    for a in &animations {
                        if a.effect == Effect::Play {
                            l.videos.play(scene, a.node, now_ms);
                        }
                    }
                }
                l.timeline.push(now_ms, animations);
                l.rev += 1;
            }
            OutputCommand::SafeAreaOverlay(area) => l.safe_area = area,
            OutputCommand::Calibrate(pattern) => l.calibrate = pattern,
        }
        l.dirty = true;
        l.pending = Some(l.pending.map_or(sent, |p| p.min(sent)));
        if is_show {
            self.evict_images();
        }
    }

    /// Drops cached images that no output's current scene uses, once there
    /// are many.
    fn evict_images(&self) {
        let mut used = std::collections::HashSet::new();
        for l in self.logical.values() {
            if let Some(scene) = &l.scene {
                images_in(&scene.nodes, &mut used);
            }
        }
        self.renderer
            .evict_images(IMAGE_BUDGET, |blob| used.contains(blob));
    }

    fn configure(
        &self,
        surface: wgpu::Surface<'static>,
        (width, height): (u32, u32),
        background: Background,
    ) -> Result<Sink, RenderError> {
        let gpu = &self.renderer.gpu;
        let caps = surface.get_capabilities(&gpu.adapter);
        let mut config = surface
            .get_default_config(&gpu.adapter, width, height)
            .ok_or_else(|| RenderError::Surface("the GPU cannot present to this window".into()))?;
        // Scene textures hold sRGB-encoded values already, so the surface
        // view must not encode again.
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| {
                matches!(
                    f,
                    wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm
                )
            })
            .unwrap_or(config.format);
        let view_format = format.remove_srgb_suffix();
        config.format = format;
        config.view_formats = if view_format == format {
            vec![]
        } else {
            vec![view_format]
        };
        config.usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
        config.present_mode = wgpu::PresentMode::Fifo;
        config.desired_maximum_frame_latency = self.options.frame_latency.max(1);
        let wanted = match background {
            Background::Opaque => wgpu::CompositeAlphaMode::Opaque,
            Background::Transparent => wgpu::CompositeAlphaMode::PreMultiplied,
        };
        if caps.alpha_modes.contains(&wanted) {
            config.alpha_mode = wanted;
        }
        surface.configure(&gpu.device, &config);
        Ok(Sink::Surface {
            surface,
            config,
            view_format,
        })
    }

    /// Opens and drops players to match what the outputs show, and moves
    /// each to `now_ms`; outputs whose video has a new frame are marked.
    fn advance_videos(&mut self, now_ms: f64) {
        let wanted: HashSet<PlayerKey> = self
            .logical
            .values()
            .flat_map(|l| l.videos.players().map(|(_, key)| key))
            .collect();
        self.players.sync(&wanted, &self.renderer.blobs);
        let changed = self.players.advance(now_ms, &self.renderer.gpu);
        if changed.is_empty() {
            return;
        }
        for l in self.logical.values_mut() {
            if l.videos.players().any(|(_, key)| changed.contains(&key)) {
                l.video_frame = true;
            }
        }
    }

    fn render_all(&mut self) {
        let now_ms = self.now_ms();
        self.advance_videos(now_ms);
        let now = Instant::now();
        let ids: Vec<OutputId> = self
            .attached
            .iter()
            .filter(|(_, a)| a.retry_at.is_none_or(|at| at <= now))
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            if !self.needs_frame(&id, now_ms) {
                continue;
            }
            // One output's bad scene must not take the others down: a panic
            // drawing it resets only that output.
            if let Err(panic) = catch_unwind(AssertUnwindSafe(|| self.render_output(id))) {
                let message = panic_message(&*panic);
                self.recover(Some(id), message);
            }
        }
    }

    /// Draws `id`'s scene textures now if they are out of date.
    fn draw_pending(&mut self, id: OutputId, now_ms: f64) {
        let (Some(a), Some(l)) = (self.attached.get(&id), self.logical.get(&id)) else {
            return;
        };
        if l.scene.is_none() || (a.work.incoming.is_some() && a.work.incoming_rev == l.rev) {
            return;
        }
        let device = &self.renderer.gpu.device;
        let mut encoder = device.create_command_encoder(&Default::default());
        self.prepare(id, now_ms, &mut encoder);
        self.renderer.gpu.queue.submit([encoder.finish()]);
    }

    /// Brings `id`'s scene textures up to date, recording any drawing into
    /// `encoder`.
    fn prepare(&mut self, id: OutputId, now_ms: f64, encoder: &mut wgpu::CommandEncoder) {
        let Engine {
            renderer,
            logical,
            attached,
            players,
            ..
        } = self;
        let (Some(a), Some(l)) = (attached.get_mut(&id), logical.get_mut(&id)) else {
            return;
        };
        let live: HashMap<VideoKey, FrameView> = l
            .videos
            .players()
            .filter_map(|(node, key)| Some((node, players.frame(&key)?.clone())))
            .collect();
        let none = HashMap::new();
        let no_videos = VideoFrames::Live(&none);
        let (w, h) = a.size;
        let device = &renderer.gpu.device;
        let take = |slot: Option<Texture>, spare: &mut Option<Texture>| {
            slot.or_else(|| spare.take())
                .filter(|t| t.size() == (w, h))
                .unwrap_or_else(|| Texture::new(device, "output scene", w, h, 1, 1))
        };

        if let Some((t, start)) = l.transition {
            if progress((now_ms - start) as f32, t.duration_ms) >= 1.0 {
                l.transition = None;
                a.work.spare = a.work.outgoing.take();
            }
        }

        if let Some(pattern) = l.calibrate.clone() {
            if a.work
                .calibration
                .as_ref()
                .is_none_or(|(p, _)| *p != pattern)
            {
                let texture = take(a.work.calibration.take().map(|c| c.1), &mut None);
                let scene = overlay::calibration_scene(&pattern, w as f32, h as f32);
                log_error(renderer.draw(
                    &scene,
                    w,
                    h,
                    &texture.view,
                    encoder,
                    &HashMap::new(),
                    no_videos,
                ));
                a.work.calibration = Some((pattern, texture));
            }
            l.video_frame = false;
            return;
        }
        a.work.calibration = None;

        match &l.scene {
            Some(scene) => {
                let running = l.timeline.running(now_ms);
                let stale = a.work.incoming.is_none()
                    || a.work.incoming_rev != l.rev
                    || a.work.incoming_live
                    || l.video_frame
                    || running;
                l.video_frame = false;
                if stale {
                    let texture = take(a.work.incoming.take(), &mut a.work.spare);
                    let anims = l.timeline.state(now_ms);
                    let videos = VideoFrames::Live(&live);
                    log_error(renderer.draw(scene, w, h, &texture.view, encoder, &anims, videos));
                    a.work.incoming = Some(texture);
                    a.work.incoming_rev = l.rev;
                    a.work.incoming_live = running;
                }
            }
            None => {
                if let Some(t) = a.work.incoming.take() {
                    a.work.spare = Some(t);
                }
                a.work.incoming_live = false;
            }
        }

        match l.safe_area {
            Some(area) => {
                if a.work.overlay.as_ref().is_none_or(|(o, _)| *o != area) {
                    let texture = take(a.work.overlay.take().map(|o| o.1), &mut None);
                    let scene = overlay::safe_area_scene(&area, w as f32, h as f32);
                    log_error(renderer.draw(
                        &scene,
                        w,
                        h,
                        &texture.view,
                        encoder,
                        &HashMap::new(),
                        no_videos,
                    ));
                    a.work.overlay = Some((area, texture));
                }
            }
            None => a.work.overlay = None,
        }
    }

    /// Composites `id`'s current picture into `target`.
    fn composite(
        &mut self,
        id: OutputId,
        now_ms: f64,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        format: wgpu::TextureFormat,
    ) {
        let Engine {
            renderer,
            compositor,
            logical,
            attached,
            ..
        } = self;
        let (Some(a), Some(l)) = (attached.get(&id), logical.get(&id)) else {
            return;
        };
        let full = |which| {
            transition_layers(TransitionKind::Cut, 1.0)
                .into_iter()
                .map(move |layer| CompositeLayer { which, ..layer })
        };
        let mut layers: Vec<(&wgpu::TextureView, CompositeLayer)> = Vec::new();
        if let Some((_, texture)) = &a.work.calibration {
            layers.extend(full(Which::Incoming).map(|layer| (&texture.view, layer)));
        } else {
            let plan = match l.transition {
                Some((t, start)) => {
                    transition_layers(t.kind, progress((now_ms - start) as f32, t.duration_ms))
                }
                None => transition_layers(TransitionKind::Cut, 1.0),
            };
            for layer in plan {
                let texture = match layer.which {
                    Which::Incoming => &a.work.incoming,
                    Which::Outgoing => &a.work.outgoing,
                };
                if let Some(texture) = texture {
                    layers.push((&texture.view, layer));
                }
            }
            if let Some((_, texture)) = &a.work.overlay {
                layers.extend(full(Which::Incoming).map(|layer| (&texture.view, layer)));
            }
        }
        let clear = match a.background {
            Background::Opaque => wgpu::Color::BLACK,
            Background::Transparent => wgpu::Color::TRANSPARENT,
        };
        compositor.draw(
            &renderer.gpu,
            &renderer.pipelines,
            encoder,
            target,
            format,
            a.size,
            clear,
            &layers,
        );
    }

    fn render_output(&mut self, id: OutputId) {
        let now_ms = self.now_ms();
        let Some(a) = self.attached.get_mut(&id) else {
            return;
        };
        if std::mem::take(&mut a.panic_next) {
            panic!("injected panic while drawing an output");
        }
        self.logical.entry(id).or_default();
        let device = self.renderer.gpu.device.clone();
        let queue = self.renderer.gpu.queue.clone();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("output frame"),
        });
        self.prepare(id, now_ms, &mut encoder);

        enum Acquired {
            Headless(wgpu::TextureView),
            Surface(wgpu::SurfaceTexture, bool, wgpu::TextureFormat),
            Skip { occluded: bool },
            Outdated,
            Lost(&'static str),
        }
        let Some(a) = self.attached.get(&id) else {
            return;
        };
        let acquired = match &a.sink {
            Sink::Headless { texture } => Acquired::Headless(texture.view.clone()),
            Sink::Surface {
                surface,
                view_format,
                ..
            } => match surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(t) => {
                    Acquired::Surface(t, false, *view_format)
                }
                wgpu::CurrentSurfaceTexture::Suboptimal(t) => {
                    Acquired::Surface(t, true, *view_format)
                }
                wgpu::CurrentSurfaceTexture::Timeout => Acquired::Skip { occluded: false },
                wgpu::CurrentSurfaceTexture::Occluded => Acquired::Skip { occluded: true },
                wgpu::CurrentSurfaceTexture::Outdated => Acquired::Outdated,
                wgpu::CurrentSurfaceTexture::Lost => Acquired::Lost("surface lost"),
                _ => Acquired::Lost("surface validation error"),
            },
        };
        match acquired {
            Acquired::Headless(view) => {
                self.composite(id, now_ms, &mut encoder, &view, FORMAT);
                queue.submit([encoder.finish()]);
                self.presented(id);
                if let Some(a) = self.attached.get_mut(&id) {
                    a.retry_at = Some(Instant::now() + HEADLESS_FRAME);
                }
                return;
            }
            Acquired::Surface(frame, suboptimal, view_format) => {
                let view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
                    format: Some(view_format),
                    ..Default::default()
                });
                self.composite(id, now_ms, &mut encoder, &view, view_format);
                queue.submit([encoder.finish()]);
                queue.present(frame);
                if suboptimal {
                    self.reconfigure(id);
                }
            }
            other => {
                // The scene textures drawn above stay valid; submit them.
                queue.submit([encoder.finish()]);
                match other {
                    Acquired::Skip { occluded } => self.skip(id, occluded),
                    Acquired::Outdated => {
                        self.reconfigure(id);
                        self.skip(id, false);
                    }
                    Acquired::Lost(reason) => self.lose(id, reason.into()),
                    _ => {}
                }
                return;
            }
        }
        self.presented(id);
    }

    fn presented(&mut self, id: OutputId) {
        let at = Instant::now();
        let Some(a) = self.attached.get_mut(&id) else {
            return;
        };
        a.frame += 1;
        a.retry_at = None;
        let frame = a.frame;
        let first = !std::mem::replace(&mut a.shown, true);
        let pending = self.logical.get_mut(&id).and_then(|l| {
            l.dirty = false;
            l.pending.take()
        });
        if first {
            self.emit(OutputEvent::FirstFrame(id));
        }
        if let Some(sent) = pending {
            self.emit(OutputEvent::Presented(FrameTiming {
                output: id,
                frame,
                sent,
                presented: at,
            }));
        }
    }

    /// The surface had no texture to give (occluded, timed out).
    ///
    /// macOS reports a hidden window as occluded and will not present to
    /// it (wgpu skips `nextDrawable` there, which would otherwise hang). The
    /// first frame's scene textures are drawn by now, so the window is shown
    /// at once and the frame goes out on the next retry, one interval
    /// later; the window's own background (black) covers that gap.
    fn skip(&mut self, id: OutputId, occluded: bool) {
        let now = Instant::now();
        let Some(a) = self.attached.get_mut(&id) else {
            return;
        };
        a.retry_at = Some(now + RETRY);
        if !a.shown && (occluded || now - a.attached_at > FIRST_FRAME_FALLBACK) {
            a.shown = true;
            self.emit(OutputEvent::FirstFrame(id));
        }
    }

    fn reconfigure(&mut self, id: OutputId) {
        let device = &self.renderer.gpu.device;
        if let Some(Attached {
            sink: Sink::Surface {
                surface, config, ..
            },
            ..
        }) = self.attached.get_mut(&id)
        {
            surface.configure(device, config);
        }
    }

    fn lose(&mut self, id: OutputId, reason: String) {
        if let Some(a) = self.attached.get_mut(&id) {
            a.lost = true;
        }
        self.emit(OutputEvent::SurfaceLost { output: id, reason });
    }

    fn snapshot(&mut self, id: OutputId) -> Result<Image, RenderError> {
        let Some(a) = self.attached.get(&id) else {
            return Err(RenderError::Surface("no such output".into()));
        };
        let (w, h) = a.size;
        let now_ms = self.now_ms();
        self.logical.entry(id).or_default();
        let device = self.renderer.gpu.device.clone();
        let mut encoder = device.create_command_encoder(&Default::default());
        self.prepare(id, now_ms, &mut encoder);
        let target = Texture::new(&device, "snapshot", w, h, 1, 1);
        self.composite(id, now_ms, &mut encoder, &target.view, FORMAT);
        self.renderer.read_back(&target, encoder)
    }

    /// Recovers from a panic: drops the GPU working state of `output` (or
    /// of every output), finishes animations and transitions, and draws the
    /// last scene again. A scene that panics twice in a row is dropped so
    /// the output shows black instead of crashing forever.
    fn recover(&mut self, output: Option<OutputId>, message: String) {
        let now = Instant::now();
        let targets: Vec<OutputId> = match output {
            Some(id) => vec![id],
            None => {
                let mut all: Vec<OutputId> = self.logical.keys().copied().collect();
                all.extend(
                    self.attached
                        .keys()
                        .filter(|id| !self.logical.contains_key(id)),
                );
                all
            }
        };
        let repeated = match output {
            Some(_) => false,
            None => {
                self.thread_panics.retain(|t| now - *t < PANIC_WINDOW);
                self.thread_panics.push(now);
                self.thread_panics.len() >= 3
            }
        };
        let mut blanked = false;
        for id in targets {
            if let Some(a) = self.attached.get_mut(&id) {
                a.work = Work::default();
                a.retry_at = None;
                a.panic_next = false;
            }
            if let Some(l) = self.logical.get_mut(&id) {
                l.transition = None;
                l.timeline.finish_all();
                l.rev += 1;
                l.dirty = true;
                l.panics.retain(|t| now - *t < PANIC_WINDOW);
                if output.is_some() {
                    l.panics.push(now);
                }
                if repeated || l.panics.len() >= 2 {
                    l.scene = None;
                    l.videos = VideoClock::default();
                    l.timeline = AnimationTimeline::default();
                    l.calibrate = None;
                    blanked = true;
                }
            }
        }
        eprintln!("lampstand-render: render thread panicked ({message}); restarted");
        self.emit(OutputEvent::Restarted {
            output,
            message,
            blanked,
        });
    }
}

fn images_in(nodes: &[lampstand_core::scene::Node], out: &mut std::collections::HashSet<BlobHash>) {
    use lampstand_core::scene::NodeKind;
    for node in nodes {
        match &node.kind {
            NodeKind::Image(image) => {
                out.insert(image.blob);
            }
            NodeKind::Group(children) => images_in(children, out),
            _ => {}
        }
    }
}

fn log_error(result: Result<(), RenderError>) {
    if let Err(e) = result {
        eprintln!("lampstand-render: drawing a scene failed: {e:?}");
    }
}
