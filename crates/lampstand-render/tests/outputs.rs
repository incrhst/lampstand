//! The render thread, end to end, on headless outputs: commands in,
//! composited pictures out (`Outputs::snapshot`). No display needed; where
//! there is no GPU adapter at all, each test prints that it skipped.
//!
//! Transition and animation timelines themselves are unit-tested in
//! `output::timeline`; these tests check the thread wires them up, recovers
//! from panics and keeps outputs independent.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use lampstand_core::geom::{Color, Point, Transform};
use lampstand_core::id::BlobHash;
use lampstand_core::output::OutputId;
use lampstand_core::safe_area::SafeArea;
use lampstand_core::scene::*;
use lampstand_render::{
    AdapterChoice, BlobSource, Image, OutputCommand, OutputEvent, OutputOptions, OutputSink,
    Outputs, RenderError, Renderer, RendererOptions,
};

struct NoBlobs;

impl BlobSource for NoBlobs {
    fn path(&self, _: &BlobHash) -> Option<PathBuf> {
        None
    }
}

const W: u32 = 160;
const H: u32 = 90;

fn start() -> Option<(Outputs, Receiver<OutputEvent>)> {
    let options = RendererOptions {
        adapter: AdapterChoice::PreferIntegrated,
        system_fonts: false,
    };
    let renderer = match Renderer::with_options(Arc::new(NoBlobs), options) {
        Ok(r) => r,
        Err(RenderError::NoAdapter) => {
            eprintln!("skipped: no GPU adapter");
            return None;
        }
        Err(e) => panic!("renderer: {e:?}"),
    };
    let (tx, rx) = mpsc::channel();
    let outputs = Outputs::spawn(renderer, OutputOptions::default(), move |e| {
        let _ = tx.send(e);
    })
    .unwrap();
    Some((outputs, rx))
}

fn rect(id: NodeId, x: f32, y: f32, w: f32, h: f32, color: Color) -> Node {
    Node {
        id,
        transform: Transform::IDENTITY,
        opacity: 1.0,
        visible: true,
        kind: NodeKind::Path(PathNode {
            commands: vec![
                PathCommand::MoveTo(Point { x, y }),
                PathCommand::LineTo(Point { x: x + w, y }),
                PathCommand::LineTo(Point { x: x + w, y: y + h }),
                PathCommand::LineTo(Point { x, y: y + h }),
                PathCommand::Close,
            ],
            fill: Some(Paint::Solid(color)),
            stroke: None,
            shadow: None,
        }),
    }
}

fn solid(color: Color) -> Arc<Scene> {
    Arc::new(Scene {
        width: W as f32,
        height: H as f32,
        nodes: vec![rect(1, 0.0, 0.0, W as f32, H as f32, color)],
    })
}

const RED: Color = Color::rgb(255, 0, 0);
const BLUE: Color = Color::rgb(0, 0, 255);
const GREEN: Color = Color::rgb(0, 255, 0);

fn px(image: &Image, x: u32, y: u32) -> [u8; 4] {
    let i = 4 * (y * image.width + x) as usize;
    image.pixels[i..i + 4].try_into().unwrap()
}

fn near(a: [u8; 4], b: Color) -> bool {
    let d = |x: u8, y: u8| x.abs_diff(y) <= 8;
    d(a[0], b.r) && d(a[1], b.g) && d(a[2], b.b) && d(a[3], b.a)
}

fn show(outputs: &Outputs, id: OutputId, scene: Arc<Scene>, transition: Transition) {
    outputs.send(id, OutputCommand::Show { scene, transition });
}

fn wait_for(rx: &Receiver<OutputEvent>, what: impl Fn(&OutputEvent) -> bool) -> OutputEvent {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let event = rx
            .recv_timeout(left)
            .expect("timed out waiting for an event");
        if what(&event) {
            return event;
        }
    }
}

#[test]
fn shows_scenes_and_cuts_between_them() {
    let Some((outputs, rx)) = start() else { return };
    let id = OutputId::generate();
    outputs.attach_headless(id, W, H).unwrap();
    wait_for(&rx, |e| *e == OutputEvent::FirstFrame(id));
    // Nothing shown yet: black.
    let image = outputs.snapshot(id).unwrap();
    assert!(near(px(&image, 10, 10), Color::BLACK));

    show(&outputs, id, solid(RED), Transition::CUT);
    assert!(near(px(&outputs.snapshot(id).unwrap(), 80, 45), RED));
    show(&outputs, id, solid(BLUE), Transition::CUT);
    assert!(near(px(&outputs.snapshot(id).unwrap(), 80, 45), BLUE));
}

#[test]
fn commands_sent_before_attach_show_in_the_first_frame() {
    let Some((outputs, _rx)) = start() else {
        return;
    };
    let id = OutputId::generate();
    show(&outputs, id, solid(GREEN), Transition::CUT);
    outputs.attach_headless(id, W, H).unwrap();
    assert!(near(px(&outputs.snapshot(id).unwrap(), 5, 5), GREEN));
}

#[test]
fn fade_blends_then_settles() {
    let Some((outputs, _rx)) = start() else {
        return;
    };
    let id = OutputId::generate();
    outputs.attach_headless(id, W, H).unwrap();
    show(&outputs, id, solid(RED), Transition::CUT);
    let fade = Transition {
        kind: TransitionKind::Fade,
        duration_ms: 1500,
    };
    show(&outputs, id, solid(BLUE), fade);
    std::thread::sleep(Duration::from_millis(600));
    let mid = px(&outputs.snapshot(id).unwrap(), 80, 45);
    assert!(mid[0] > 40 && mid[2] > 40, "a mix of red and blue: {mid:?}");
    assert_eq!(mid[3], 255, "a cross-fade of opaque scenes stays opaque");
    std::thread::sleep(Duration::from_millis(1100));
    assert!(near(px(&outputs.snapshot(id).unwrap(), 80, 45), BLUE));
}

#[test]
fn push_shows_both_scenes_side_by_side() {
    let Some((outputs, _rx)) = start() else {
        return;
    };
    let id = OutputId::generate();
    outputs.attach_headless(id, W, H).unwrap();
    show(&outputs, id, solid(RED), Transition::CUT);
    let push = Transition {
        kind: TransitionKind::Push(Direction::Right),
        duration_ms: 2000,
    };
    show(&outputs, id, solid(BLUE), push);
    std::thread::sleep(Duration::from_millis(1000));
    let image = outputs.snapshot(id).unwrap();
    // The incoming scene enters from the right.
    assert!(near(px(&image, 2, 45), RED), "{:?}", px(&image, 2, 45));
    assert!(
        near(px(&image, W - 3, 45), BLUE),
        "{:?}",
        px(&image, W - 3, 45)
    );
}

#[test]
fn build_steps_reveal_nodes() {
    let Some((outputs, _rx)) = start() else {
        return;
    };
    let id = OutputId::generate();
    outputs.attach_headless(id, W, H).unwrap();
    let mut box_node = rect(7, 40.0, 20.0, 80.0, 50.0, GREEN);
    box_node.visible = false;
    let scene = Arc::new(Scene {
        width: W as f32,
        height: H as f32,
        nodes: vec![box_node],
    });
    show(&outputs, id, scene, Transition::CUT);
    assert!(near(
        px(&outputs.snapshot(id).unwrap(), 80, 45),
        Color::BLACK
    ));

    let step = |effect, duration_ms| Animation {
        node: 7,
        effect,
        delay_ms: 0,
        duration_ms,
        easing: Easing::Linear,
    };
    outputs.send(
        id,
        OutputCommand::Animate(vec![step(Effect::WipeIn(Direction::Left), 2000)]),
    );
    std::thread::sleep(Duration::from_millis(1000));
    let image = outputs.snapshot(id).unwrap();
    assert!(near(px(&image, 45, 45), GREEN), "left part revealed");
    assert!(
        near(px(&image, 115, 45), Color::BLACK),
        "right part not yet"
    );

    // The next step finishes the wipe at once and spins the box 90°: a
    // wide box turned upright covers pixels above and below it.
    outputs.send(
        id,
        OutputCommand::Animate(vec![step(Effect::Spin(90.0), 0)]),
    );
    let image = outputs.snapshot(id).unwrap();
    assert!(near(px(&image, 80, 10), GREEN), "{:?}", px(&image, 80, 10));
    assert!(
        near(px(&image, 45, 45), Color::BLACK),
        "{:?}",
        px(&image, 45, 45)
    );
}

#[test]
fn safe_area_overlay_and_calibration_draw_over_the_scene() {
    let Some((outputs, _rx)) = start() else {
        return;
    };
    let id = OutputId::generate();
    outputs.attach_headless(id, W, H).unwrap();
    show(&outputs, id, solid(BLUE), Transition::CUT);
    let area = SafeArea {
        top: 0.1,
        right: 0.1,
        bottom: 0.1,
        left: 0.1,
    };
    outputs.send(id, OutputCommand::SafeAreaOverlay(Some(area)));
    let image = outputs.snapshot(id).unwrap();
    // A thin dashed line: look for yellow near the left margin (x = 16).
    let yellowish = (14..19)
        .flat_map(|x| (20..70).map(move |y| (x, y)))
        .any(|(x, y)| {
            let p = px(&image, x, y);
            p[0] > 100 && p[1] > 80
        });
    assert!(yellowish, "outline at the margin");
    assert!(near(px(&image, 80, 45), BLUE));

    outputs.send(
        id,
        OutputCommand::Calibrate(Some(lampstand_render::CalibrationPattern {
            current: area,
            max_percent: 10,
            active: None,
            label: "Test".into(),
        })),
    );
    let image = outputs.snapshot(id).unwrap();
    assert!(
        near(px(&image, 40, 20), Color::BLACK),
        "calibration replaces the scene"
    );
    outputs.send(id, OutputCommand::Calibrate(None));
    outputs.send(id, OutputCommand::SafeAreaOverlay(None));
    let image = outputs.snapshot(id).unwrap();
    assert!(near(px(&image, 16, 45), BLUE));
}

#[test]
fn a_panic_on_one_output_restarts_it_with_the_last_scene() {
    let Some((outputs, rx)) = start() else { return };
    let a = OutputId::generate();
    let b = OutputId::generate();
    outputs.attach_headless(a, W, H).unwrap();
    outputs.attach_headless(b, W, H).unwrap();
    show(&outputs, a, solid(RED), Transition::CUT);
    show(&outputs, b, solid(GREEN), Transition::CUT);
    assert!(near(px(&outputs.snapshot(a).unwrap(), 5, 5), RED));

    outputs.inject_panic(Some(a));
    // Any command makes `a` draw a frame, which panics.
    show(&outputs, a, solid(BLUE), Transition::CUT);
    let event = wait_for(&rx, |e| matches!(e, OutputEvent::Restarted { .. }));
    assert!(matches!(
        event,
        OutputEvent::Restarted { output: Some(o), blanked: false, .. } if o == a
    ));
    assert!(
        near(px(&outputs.snapshot(a).unwrap(), 5, 5), BLUE),
        "last scene again"
    );
    assert!(
        near(px(&outputs.snapshot(b).unwrap(), 5, 5), GREEN),
        "b untouched"
    );

    // A second panic soon after blanks the output instead of looping.
    outputs.inject_panic(Some(a));
    show(&outputs, a, solid(RED), Transition::CUT);
    let event = wait_for(&rx, |e| matches!(e, OutputEvent::Restarted { .. }));
    assert!(matches!(
        event,
        OutputEvent::Restarted { blanked: true, .. }
    ));
    assert!(near(px(&outputs.snapshot(a).unwrap(), 5, 5), Color::BLACK));
    assert!(near(px(&outputs.snapshot(b).unwrap(), 5, 5), GREEN));
}

#[test]
fn a_render_thread_panic_is_survived() {
    let Some((outputs, rx)) = start() else { return };
    let id = OutputId::generate();
    outputs.attach_headless(id, W, H).unwrap();
    show(&outputs, id, solid(RED), Transition::CUT);
    let fade = Transition {
        kind: TransitionKind::Fade,
        duration_ms: 60_000,
    };
    show(&outputs, id, solid(GREEN), fade);
    outputs.inject_panic(None);
    let event = wait_for(&rx, |e| matches!(e, OutputEvent::Restarted { .. }));
    assert!(matches!(event, OutputEvent::Restarted { output: None, .. }));
    // The transition in progress jumps to its end.
    let p = px(&outputs.snapshot(id).unwrap(), 5, 5);
    assert!(near(p, GREEN), "{p:?}");
}

#[test]
fn detached_outputs_keep_their_scene_for_the_next_attach() {
    let Some((outputs, _rx)) = start() else {
        return;
    };
    let id = OutputId::generate();
    outputs.attach_headless(id, W, H).unwrap();
    show(&outputs, id, solid(RED), Transition::CUT);
    outputs.detach(id);
    assert!(outputs.snapshot(id).is_err());
    outputs.attach_headless(id, 2 * W, 2 * H).unwrap();
    let image = outputs.snapshot(id).unwrap();
    assert_eq!((image.width, image.height), (2 * W, 2 * H));
    assert!(near(px(&image, 300, 170), RED));
}

/// Command to presented frame on a headless output: everything but the
/// display's own refresh. The window test (`tests/windows.rs`) measures the
/// same on real surfaces.
#[test]
fn send_to_present_is_fast() {
    let Some((outputs, rx)) = start() else { return };
    let id = OutputId::generate();
    outputs.attach_headless(id, 1920, 1080).unwrap();
    wait_for(&rx, |e| *e == OutputEvent::FirstFrame(id));
    let mut latencies = Vec::new();
    for i in 0..20 {
        let color = if i % 2 == 0 { RED } else { BLUE };
        show(&outputs, id, solid(color), Transition::CUT);
        let OutputEvent::Presented(timing) =
            wait_for(&rx, |e| matches!(e, OutputEvent::Presented(_)))
        else {
            unreachable!()
        };
        latencies.push(timing.latency());
    }
    latencies.sort();
    let median = latencies[latencies.len() / 2];
    eprintln!(
        "headless send→present at 1080p: median {median:?}, max {:?}",
        latencies.last().unwrap()
    );
    // Generous: software adapters in CI are slow. The real budget is 50 ms
    // click to photon including composition and one display refresh.
    assert!(median < Duration::from_millis(250), "median {median:?}");
}

/// The bundled 2-second test clip (160×90, 30 fps H.264) as blob "clip".
struct Clip;

impl BlobSource for Clip {
    fn path(&self, blob: &BlobHash) -> Option<PathBuf> {
        (*blob == BlobHash::of(b"clip")).then(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../lampstand-media/tests/fixtures/clip-2s.mp4")
        })
    }
}

fn start_with_clip() -> Option<Outputs> {
    let options = RendererOptions {
        adapter: AdapterChoice::PreferIntegrated,
        system_fonts: false,
    };
    let renderer = match Renderer::with_options(Arc::new(Clip), options) {
        Ok(r) => r,
        Err(RenderError::NoAdapter) => {
            eprintln!("skipped: no GPU adapter");
            return None;
        }
        Err(e) => panic!("renderer: {e:?}"),
    };
    Some(Outputs::spawn(renderer, OutputOptions::default(), |_| {}).unwrap())
}

fn video_scene(looped: bool, cover: Option<Color>) -> Arc<Scene> {
    let mut nodes = vec![Node {
        id: 1,
        transform: Transform::IDENTITY,
        opacity: 1.0,
        visible: true,
        kind: NodeKind::Video(VideoNode {
            blob: BlobHash::of(b"clip"),
            rect: lampstand_core::geom::Rect {
                x: 0.0,
                y: 0.0,
                width: W as f32,
                height: H as f32,
            },
            fit: Fit::Contain,
            looped,
            muted: true,
            autoplay: true,
        }),
    }];
    if let Some(color) = cover {
        nodes.push(rect(2, 0.0, 0.0, W as f32, H as f32, color));
    }
    Arc::new(Scene {
        width: W as f32,
        height: H as f32,
        nodes,
    })
}

/// Pixels that are not black.
fn lit(image: &Image) -> usize {
    image
        .pixels
        .chunks(4)
        .filter(|p| p[0] > 24 || p[1] > 24 || p[2] > 24)
        .count()
}

/// Snapshots until `ok` holds, for up to 10 s.
fn snapshot_until(outputs: &Outputs, id: OutputId, ok: impl Fn(&Image) -> bool) -> Image {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let image = outputs.snapshot(id).unwrap();
        if ok(&image) || Instant::now() > deadline {
            return image;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_video_plays_on_an_output() {
    let Some(outputs) = start_with_clip() else {
        return;
    };
    let id = OutputId::generate();
    outputs.attach_headless(id, W, H).unwrap();
    show(&outputs, id, video_scene(false, None), Transition::CUT);
    let first = snapshot_until(&outputs, id, |i| lit(i) > (W * H / 2) as usize);
    assert!(lit(&first) > (W * H / 2) as usize, "the video never drew");
    // testsrc2 moves every frame: a later snapshot differs.
    let later = snapshot_until(&outputs, id, |i| i.pixels != first.pixels);
    assert_ne!(later.pixels, first.pixels, "the video did not advance");
}

#[test]
fn black_covers_a_playing_video_and_uncovering_shows_it_again() {
    let Some(outputs) = start_with_clip() else {
        return;
    };
    let id = OutputId::generate();
    outputs.attach_headless(id, W, H).unwrap();
    show(&outputs, id, video_scene(true, None), Transition::CUT);
    snapshot_until(&outputs, id, |i| lit(i) > (W * H / 2) as usize);
    show(
        &outputs,
        id,
        video_scene(true, Some(Color::BLACK)),
        Transition::CUT,
    );
    let black = outputs.snapshot(id).unwrap();
    assert_eq!(lit(&black), 0);
    std::thread::sleep(Duration::from_millis(100));
    show(&outputs, id, video_scene(true, None), Transition::CUT);
    let back = snapshot_until(&outputs, id, |i| lit(i) > (W * H / 2) as usize);
    assert!(lit(&back) > (W * H / 2) as usize);
}

#[test]
fn offscreen_rendering_shows_a_videos_first_frame() {
    let options = RendererOptions {
        adapter: AdapterChoice::PreferIntegrated,
        system_fonts: false,
    };
    let renderer = match Renderer::with_options(Arc::new(Clip), options) {
        Ok(r) => r,
        Err(RenderError::NoAdapter) => return eprintln!("skipped: no GPU adapter"),
        Err(e) => panic!("renderer: {e:?}"),
    };
    let image = renderer
        .render_offscreen(&video_scene(false, None), W, H)
        .unwrap();
    assert!(lit(&image) > (W * H / 2) as usize);
}
