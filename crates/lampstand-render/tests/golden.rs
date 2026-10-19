//! Golden images: each test renders a scene with `render_offscreen` and
//! compares it with `tests/golden/<name>.png`, pixel by pixel, within a
//! tolerance (GPUs differ slightly in rasterisation and filtering).
//!
//! To accept new output (after an intended change, or for a new test):
//!
//! LAMPSTAND_BLESS=1 cargo test -p lampstand-render --test golden
//!
//! and review the PNGs before committing them. A failing test writes what
//! it got and a diff image to `target/golden/`.
//!
//! Where no adapter exists, every test here passes after printing that it
//! skipped. CI installs lavapipe on Linux; Windows has WARP.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use lampstand_core::geom::{Color, Point, Rect, Transform};
use lampstand_core::id::BlobHash;
use lampstand_core::scene::*;
use lampstand_render::{AdapterChoice, BlobSource, Image, RenderError, Renderer, RendererOptions};

/// A pixel matches if no premultiplied channel differs by more than this.
const CHANNEL_TOLERANCE: u8 = 24;
/// A test passes if at most this fraction of pixels do not match.
const MAX_MISMATCHED: f64 = 0.003;

struct Blobs {
    dir: tempfile::TempDir,
}

impl BlobSource for Blobs {
    fn path(&self, blob: &BlobHash) -> Option<PathBuf> {
        let path = self.dir.path().join(blob.to_string());
        path.exists().then_some(path)
    }
}

impl Blobs {
    /// Tests run in parallel and several add the same image, so write to a
    /// unique temp file and rename: a reader never sees a half-written blob.
    fn add(&self, png: &[u8]) -> BlobHash {
        let hash = BlobHash::of(png);
        let path = self.dir.path().join(hash.to_string());
        if !path.exists() {
            let tmp = tempfile::NamedTempFile::new_in(self.dir.path()).unwrap();
            std::fs::write(tmp.path(), png).unwrap();
            tmp.persist(&path).unwrap();
        }
        hash
    }
}

struct Fixture {
    renderer: Renderer,
    blobs: Arc<Blobs>,
}

fn fixture() -> Option<&'static Fixture> {
    static FIXTURE: OnceLock<Option<Fixture>> = OnceLock::new();
    FIXTURE
        .get_or_init(|| {
            let blobs = Arc::new(Blobs {
                dir: tempfile::tempdir().unwrap(),
            });
            let options = RendererOptions {
                adapter: AdapterChoice::PreferIntegrated,
                system_fonts: false,
            };
            match Renderer::with_options(blobs.clone(), options) {
                Ok(renderer) => {
                    eprintln!("golden tests render on {:?}", renderer.adapter_info());
                    Some(Fixture { renderer, blobs })
                }
                Err(RenderError::NoAdapter) => None,
                Err(e) => panic!("renderer: {e:?}"),
            }
        })
        .as_ref()
}

macro_rules! fixture_or_skip {
    () => {
        match fixture() {
            Some(f) => f,
            None => {
                eprintln!("SKIPPED: no GPU or software adapter on this machine");
                return;
            }
        }
    };
}

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn premultiplied(px: &[u8]) -> [u8; 4] {
    let a = px[3] as u32;
    [
        (px[0] as u32 * a / 255) as u8,
        (px[1] as u32 * a / 255) as u8,
        (px[2] as u32 * a / 255) as u8,
        px[3],
    ]
}

fn check(name: &str, image: &Image) {
    let path = golden_dir().join(format!("{name}.png"));
    let actual = image::RgbaImage::from_raw(image.width, image.height, image.pixels.clone())
        .expect("image size");
    if std::env::var_os("LAMPSTAND_BLESS").is_some() {
        std::fs::create_dir_all(golden_dir()).unwrap();
        actual.save(&path).unwrap();
        return;
    }
    let expected = match image::open(&path) {
        Ok(img) => img.into_rgba8(),
        Err(e) => panic!(
            "{}: {e}; run `LAMPSTAND_BLESS=1 cargo test -p lampstand-render --test golden`",
            path.display()
        ),
    };
    assert_eq!(
        expected.dimensions(),
        actual.dimensions(),
        "{name}: size changed"
    );
    let mut diff = image::RgbaImage::new(image.width, image.height);
    let (mut mismatched, mut worst) = (0usize, 0u8);
    for ((e, a), d) in expected
        .pixels()
        .zip(actual.pixels())
        .zip(diff.pixels_mut())
    {
        let (e, a) = (premultiplied(&e.0), premultiplied(&a.0));
        let delta = (0..4).map(|i| e[i].abs_diff(a[i])).max().unwrap();
        worst = worst.max(delta);
        if delta > CHANNEL_TOLERANCE {
            mismatched += 1;
            *d = image::Rgba([255, 0, 0, 255]);
        } else {
            *d = image::Rgba([delta * 8, delta * 8, delta * 8, 255]);
        }
    }
    let total = (image.width * image.height) as usize;
    eprintln!("{name}: worst channel difference {worst}, {mismatched} of {total} pixels over {CHANNEL_TOLERANCE}");
    if mismatched as f64 > total as f64 * MAX_MISMATCHED {
        let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/golden");
        std::fs::create_dir_all(&out).unwrap();
        actual.save(out.join(format!("{name}.png"))).unwrap();
        diff.save(out.join(format!("{name}.diff.png"))).unwrap();
        panic!(
            "{name}: {mismatched} of {total} pixels differ by more than {CHANNEL_TOLERANCE} \
             (worst {worst}); wrote target/golden/{name}.png and {name}.diff.png"
        );
    }
}

fn render(f: &Fixture, scene: &Scene, width: u32, height: u32) -> Image {
    f.renderer.render_offscreen(scene, width, height).unwrap()
}

// Scene helpers.

fn p(x: f32, y: f32) -> Point {
    Point { x, y }
}

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn node(kind: NodeKind) -> Node {
    Node {
        id: 0,
        transform: Transform::IDENTITY,
        opacity: 1.0,
        visible: true,
        kind,
    }
}

fn at(transform: Transform, n: Node) -> Node {
    Node { transform, ..n }
}

fn faded(opacity: f32, n: Node) -> Node {
    Node { opacity, ..n }
}

fn rect_path(r: Rect) -> Vec<PathCommand> {
    vec![
        PathCommand::MoveTo(p(r.x, r.y)),
        PathCommand::LineTo(p(r.x + r.width, r.y)),
        PathCommand::LineTo(p(r.x + r.width, r.y + r.height)),
        PathCommand::LineTo(p(r.x, r.y + r.height)),
        PathCommand::Close,
    ]
}

/// A circle from four cubics.
fn circle(cx: f32, cy: f32, r: f32) -> Vec<PathCommand> {
    let k = 0.552_284_8 * r;
    vec![
        PathCommand::MoveTo(p(cx + r, cy)),
        PathCommand::CubicTo(p(cx + r, cy + k), p(cx + k, cy + r), p(cx, cy + r)),
        PathCommand::CubicTo(p(cx - k, cy + r), p(cx - r, cy + k), p(cx - r, cy)),
        PathCommand::CubicTo(p(cx - r, cy - k), p(cx - k, cy - r), p(cx, cy - r)),
        PathCommand::CubicTo(p(cx + k, cy - r), p(cx + r, cy - k), p(cx + r, cy)),
        PathCommand::Close,
    ]
}

fn fill(commands: Vec<PathCommand>, paint: Paint) -> Node {
    node(NodeKind::Path(PathNode {
        commands,
        fill: Some(paint),
        stroke: None,
        shadow: None,
    }))
}

fn stroke(commands: Vec<PathCommand>, paint: Paint, width: f32, dash: Option<Vec<f32>>) -> Node {
    node(NodeKind::Path(PathNode {
        commands,
        fill: None,
        stroke: Some(Stroke { paint, width, dash }),
        shadow: None,
    }))
}

fn solid(r: u8, g: u8, b: u8) -> Paint {
    Paint::Solid(Color::rgb(r, g, b))
}

fn stop(offset: f32, color: Color) -> GradientStop {
    GradientStop { offset, color }
}

fn scene(width: f32, height: f32, nodes: Vec<Node>) -> Scene {
    Scene {
        width,
        height,
        nodes,
    }
}

fn background(width: f32, height: f32, color: Color) -> Node {
    fill(
        rect_path(rect(0.0, 0.0, width, height)),
        Paint::Solid(color),
    )
}

fn rotation(degrees: f32, cx: f32, cy: f32) -> Transform {
    let (s, c) = degrees.to_radians().sin_cos();
    Transform::translate(-cx, -cy)
        .then(Transform {
            a: c,
            b: s,
            c: -s,
            d: c,
            e: 0.0,
            f: 0.0,
        })
        .then(Transform::translate(cx, cy))
}

fn run(text: &str, size: f32, color: Color) -> TextRun {
    TextRun {
        text: text.into(),
        font_family: "Noto Sans".into(),
        size,
        weight: 400,
        italic: false,
        underline: false,
        strike: false,
        color,
    }
}

fn para(align: HAlign, runs: Vec<TextRun>) -> Paragraph {
    Paragraph {
        align,
        line_spacing: 1.2,
        space_before: 0.0,
        space_after: 0.0,
        indent: 0.0,
        bullet: None,
        runs,
    }
}

fn text(rect: Rect, valign: VAlign, paragraphs: Vec<Paragraph>) -> TextBlock {
    TextBlock {
        rect,
        valign,
        paragraphs,
        shadow: None,
        outline: None,
    }
}

const NAVY: Color = Color::rgb(20, 30, 70);

// Paths and paints.

#[test]
fn path_fills() {
    let f = fixture_or_skip!();
    let star: Vec<PathCommand> = (0..10)
        .map(|i| {
            let r = if i % 2 == 0 { 90.0 } else { 38.0 };
            let a = (i as f32 * 36.0 - 90.0).to_radians();
            p(420.0 + r * a.cos(), 135.0 + r * a.sin())
        })
        .enumerate()
        .map(|(i, pt)| {
            if i == 0 {
                PathCommand::MoveTo(pt)
            } else {
                PathCommand::LineTo(pt)
            }
        })
        .chain([PathCommand::Close])
        .collect();
    let quad = vec![
        PathCommand::MoveTo(p(40.0, 230.0)),
        PathCommand::QuadTo(p(120.0, 120.0), p(200.0, 230.0)),
        PathCommand::Close,
    ];
    let s = scene(
        640.0,
        360.0,
        vec![
            background(640.0, 360.0, NAVY),
            fill(
                rect_path(rect(30.0, 30.0, 180.0, 100.0)),
                solid(230, 80, 60),
            ),
            fill(circle(300.0, 260.0, 70.0), solid(250, 200, 60)),
            fill(star, solid(90, 200, 120)),
            fill(
                quad,
                Paint::Solid(Color {
                    r: 120,
                    g: 160,
                    b: 255,
                    a: 160,
                }),
            ),
        ],
    );
    check("path_fills", &render(f, &s, 640, 360));
}

#[test]
fn path_strokes_and_dashes() {
    let f = fixture_or_skip!();
    let line = |y: f32| {
        vec![
            PathCommand::MoveTo(p(40.0, y)),
            PathCommand::LineTo(p(600.0, y)),
        ]
    };
    let zigzag = vec![
        PathCommand::MoveTo(p(40.0, 300.0)),
        PathCommand::LineTo(p(120.0, 220.0)),
        PathCommand::LineTo(p(200.0, 300.0)),
        PathCommand::LineTo(p(280.0, 220.0)),
    ];
    let s = scene(
        640.0,
        360.0,
        vec![
            background(640.0, 360.0, NAVY),
            stroke(line(40.0), solid(255, 255, 255), 1.0, None),
            stroke(line(70.0), solid(255, 255, 255), 6.0, None),
            stroke(
                line(110.0),
                solid(255, 210, 80),
                8.0,
                Some(vec![30.0, 15.0]),
            ),
            stroke(
                line(150.0),
                solid(120, 220, 255),
                4.0,
                Some(vec![4.0, 8.0, 20.0, 8.0]),
            ),
            stroke(zigzag, solid(255, 120, 160), 12.0, None),
            stroke(
                circle(470.0, 260.0, 70.0),
                solid(180, 255, 140),
                6.0,
                Some(vec![20.0, 10.0]),
            ),
        ],
    );
    check("path_strokes_and_dashes", &render(f, &s, 640, 360));
}

#[test]
fn gradients() {
    let f = fixture_or_skip!();
    let linear = Paint::Linear {
        start: p(40.0, 0.0),
        end: p(300.0, 0.0),
        stops: vec![
            stop(0.0, Color::rgb(255, 60, 60)),
            stop(0.5, Color::rgb(255, 230, 60)),
            stop(1.0, Color::rgb(60, 120, 255)),
        ],
    };
    let diagonal = Paint::Linear {
        start: p(340.0, 40.0),
        end: p(600.0, 160.0),
        stops: vec![
            stop(0.0, Color::WHITE),
            stop(
                1.0,
                Color {
                    r: 255,
                    g: 255,
                    b: 255,
                    a: 0,
                },
            ),
        ],
    };
    let radial = Paint::Radial {
        center: p(170.0, 260.0),
        radius: 80.0,
        stops: vec![
            stop(0.0, Color::rgb(255, 255, 200)),
            stop(1.0, Color::rgb(200, 60, 20)),
        ],
    };
    let stroked = node(NodeKind::Path(PathNode {
        commands: circle(470.0, 260.0, 60.0),
        fill: Some(Paint::Radial {
            center: p(450.0, 240.0),
            radius: 70.0,
            stops: vec![
                stop(0.0, Color::rgb(140, 220, 255)),
                stop(1.0, Color::rgb(20, 60, 160)),
            ],
        }),
        stroke: Some(Stroke {
            paint: Paint::Linear {
                start: p(410.0, 0.0),
                end: p(530.0, 0.0),
                stops: vec![
                    stop(0.0, Color::rgb(255, 120, 0)),
                    stop(1.0, Color::rgb(255, 0, 140)),
                ],
            },
            width: 10.0,
            dash: None,
        }),
        shadow: None,
    }));
    let s = scene(
        640.0,
        360.0,
        vec![
            background(640.0, 360.0, NAVY),
            fill(rect_path(rect(40.0, 40.0, 260.0, 120.0)), linear),
            fill(rect_path(rect(340.0, 40.0, 260.0, 120.0)), diagonal),
            fill(rect_path(rect(90.0, 180.0, 160.0, 160.0)), radial),
            stroked,
        ],
    );
    check("gradients", &render(f, &s, 640, 360));
}

#[test]
fn path_shadow() {
    let f = fixture_or_skip!();
    let card = node(NodeKind::Path(PathNode {
        commands: rect_path(rect(160.0, 90.0, 320.0, 180.0)),
        fill: Some(solid(250, 250, 250)),
        stroke: Some(Stroke {
            paint: solid(40, 40, 40),
            width: 3.0,
            dash: None,
        }),
        shadow: Some(Shadow {
            color: Color {
                r: 0,
                g: 0,
                b: 0,
                a: 200,
            },
            offset: p(12.0, 16.0),
            blur: 24.0,
        }),
    }));
    let s = scene(
        640.0,
        360.0,
        vec![background(640.0, 360.0, Color::rgb(120, 170, 220)), card],
    );
    check("path_shadow", &render(f, &s, 640, 360));
}

// Images.

/// 64×32: left half a red-to-green ramp, right half blue with a white
/// diagonal, a one-pixel yellow border. Fit and crop errors show at once.
fn test_png() -> Vec<u8> {
    let img = image::RgbaImage::from_fn(64, 32, |x, y| {
        if x == 0 || y == 0 || x == 63 || y == 31 {
            image::Rgba([255, 230, 0, 255])
        } else if x < 32 {
            image::Rgba([(255 - x * 8) as u8, (x * 8) as u8, 40, 255])
        } else if (x - 32) == y {
            image::Rgba([255, 255, 255, 255])
        } else {
            image::Rgba([30, 60, 200, 255])
        }
    });
    encode_png(&img)
}

fn encode_png(img: &image::RgbaImage) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

fn image_node(blob: BlobHash, rect: Rect, fit: Fit, crop: Option<Rect>) -> Node {
    node(NodeKind::Image(ImageNode {
        blob,
        rect,
        fit,
        crop,
    }))
}

#[test]
fn image_fit_and_crop() {
    let f = fixture_or_skip!();
    let blob = f.blobs.add(&test_png());
    let frame = |r: Rect| {
        stroke(
            rect_path(r),
            solid(255, 255, 255),
            1.0,
            Some(vec![4.0, 4.0]),
        )
    };
    let boxes = [
        rect(20.0, 20.0, 180.0, 140.0),
        rect(230.0, 20.0, 180.0, 140.0),
        rect(440.0, 20.0, 180.0, 140.0),
        rect(20.0, 200.0, 180.0, 140.0),
        rect(230.0, 200.0, 180.0, 140.0),
    ];
    let mut nodes = vec![background(640.0, 360.0, NAVY)];
    nodes.extend(boxes.iter().map(|r| frame(*r)));
    nodes.extend([
        image_node(blob, boxes[0], Fit::Fill, None),
        image_node(blob, boxes[1], Fit::Contain, None),
        image_node(blob, boxes[2], Fit::Cover, None),
        // Right half only.
        image_node(blob, boxes[3], Fit::Fill, Some(rect(0.5, 0.0, 0.5, 1.0))),
        // srcRect beyond the left edge: a gap on the left.
        image_node(blob, boxes[4], Fit::Fill, Some(rect(-0.25, 0.0, 1.25, 1.0))),
    ]);
    check(
        "image_fit_and_crop",
        &render(f, &scene(640.0, 360.0, nodes), 640, 360),
    );
}

#[test]
fn image_alpha_opacity_and_rotation() {
    let f = fixture_or_skip!();
    let disc = image::RgbaImage::from_fn(100, 100, |x, y| {
        let d = ((x as f32 - 49.5).powi(2) + (y as f32 - 49.5).powi(2)).sqrt();
        let a = (255.0 * (1.0 - d / 50.0)).clamp(0.0, 255.0) as u8;
        image::Rgba([255, 200, 80, a])
    });
    let disc = f.blobs.add(&encode_png(&disc));
    let photo = f.blobs.add(&test_png());
    let checker: Vec<Node> = (0..8)
        .flat_map(|i| (0..4).map(move |j| (i, j)))
        .filter(|(i, j)| (i + j) % 2 == 0)
        .map(|(i, j)| {
            fill(
                rect_path(rect(i as f32 * 80.0, j as f32 * 90.0, 80.0, 90.0)),
                solid(90, 90, 110),
            )
        })
        .collect();
    let mut nodes = vec![background(640.0, 360.0, Color::rgb(40, 40, 50))];
    nodes.extend(checker);
    nodes.extend([
        image_node(disc, rect(40.0, 80.0, 200.0, 200.0), Fit::Fill, None),
        faded(
            0.5,
            image_node(photo, rect(260.0, 40.0, 160.0, 80.0), Fit::Fill, None),
        ),
        at(
            rotation(30.0, 500.0, 230.0),
            image_node(photo, rect(420.0, 190.0, 160.0, 80.0), Fit::Fill, None),
        ),
    ]);
    check(
        "image_alpha_opacity_and_rotation",
        &render(f, &scene(640.0, 360.0, nodes), 640, 360),
    );
}

#[test]
fn missing_image_draws_nothing() {
    let f = fixture_or_skip!();
    let s = scene(
        64.0,
        64.0,
        vec![image_node(
            BlobHash::of(b"nowhere"),
            rect(0.0, 0.0, 64.0, 64.0),
            Fit::Fill,
            None,
        )],
    );
    let image = render(f, &s, 64, 64);
    assert!(image.pixels.iter().all(|&b| b == 0));
}

// Text.

#[test]
fn text_paragraphs_runs_and_alignment() {
    let f = fixture_or_skip!();
    let white = Color::WHITE;
    let mut bold = run("bold ", 28.0, Color::rgb(255, 220, 120));
    bold.weight = 700;
    let mut italic = run("italic ", 28.0, Color::rgb(160, 220, 255));
    italic.italic = true;
    let mut under = run("underline ", 28.0, white);
    under.underline = true;
    let mut strike = run("strike", 28.0, Color::rgb(255, 140, 140));
    strike.strike = true;
    let mut big = para(HAlign::Center, vec![run("Amazing Grace", 48.0, white)]);
    big.space_after = 10.0;
    let block = text(
        rect(40.0, 20.0, 560.0, 320.0),
        VAlign::Middle,
        vec![
            big,
            para(
                HAlign::Left,
                vec![run("Mixed ", 28.0, white), bold, italic, under, strike],
            ),
            para(
                HAlign::Right,
                vec![run(
                    "Right aligned, and long enough to wrap onto a second line",
                    28.0,
                    white,
                )],
            ),
            para(
                HAlign::Justify,
                vec![run(
                    "Justified text spreads its words to both edges of the box when it wraps.",
                    24.0,
                    white,
                )],
            ),
        ],
    );
    let frame = stroke(
        rect_path(block.rect),
        solid(255, 255, 255),
        1.0,
        Some(vec![4.0, 4.0]),
    );
    let s = scene(
        640.0,
        360.0,
        vec![
            background(640.0, 360.0, NAVY),
            frame,
            node(NodeKind::Text(block)),
        ],
    );
    check(
        "text_paragraphs_runs_and_alignment",
        &render(f, &s, 640, 360),
    );
}

#[test]
fn text_valign_and_bullets() {
    let f = fixture_or_skip!();
    let white = Color::WHITE;
    let bulleted = |bullet: &str, words: &str| Paragraph {
        indent: 40.0,
        bullet: Some(bullet.into()),
        space_after: 6.0,
        ..para(HAlign::Left, vec![run(words, 22.0, white)])
    };
    let column = |x: f32, valign| {
        text(
            rect(x, 20.0, 190.0, 320.0),
            valign,
            vec![
                bulleted("•", "First point"),
                bulleted("2.", "Second point that wraps"),
                bulleted("c)", "Third"),
            ],
        )
    };
    let blocks = [
        column(20.0, VAlign::Top),
        column(225.0, VAlign::Middle),
        column(430.0, VAlign::Bottom),
    ];
    let mut nodes = vec![background(640.0, 360.0, NAVY)];
    for b in blocks {
        nodes.push(stroke(
            rect_path(b.rect),
            solid(255, 255, 255),
            1.0,
            Some(vec![4.0, 4.0]),
        ));
        nodes.push(node(NodeKind::Text(b)));
    }
    check(
        "text_valign_and_bullets",
        &render(f, &scene(640.0, 360.0, nodes), 640, 360),
    );
}

#[test]
fn text_shadow_and_outline() {
    let f = fixture_or_skip!();
    let lyric = |text_: &str| para(HAlign::Center, vec![run(text_, 56.0, Color::WHITE)]);
    let mut block = text(
        rect(0.0, 0.0, 960.0, 540.0),
        VAlign::Middle,
        vec![lyric("How great Thou art"), lyric("How great Thou art")],
    );
    block.outline = Some(Outline {
        color: Color::BLACK,
        width: 3.0,
    });
    block.shadow = Some(Shadow {
        color: Color {
            r: 0,
            g: 0,
            b: 0,
            a: 180,
        },
        offset: p(4.0, 6.0),
        blur: 8.0,
    });
    let backdrop = fill(
        rect_path(rect(0.0, 0.0, 960.0, 540.0)),
        Paint::Linear {
            start: p(0.0, 0.0),
            end: p(960.0, 540.0),
            stops: vec![
                stop(0.0, Color::rgb(250, 220, 160)),
                stop(1.0, Color::rgb(120, 180, 230)),
            ],
        },
    );
    let s = scene(960.0, 540.0, vec![backdrop, node(NodeKind::Text(block))]);
    // Half scale: scene units are not pixels.
    check("text_shadow_and_outline", &render(f, &s, 480, 270));
}

#[test]
fn text_rotated_and_scaled() {
    let f = fixture_or_skip!();
    let label = |s: &str| {
        let mut b = text(
            rect(0.0, 0.0, 240.0, 60.0),
            VAlign::Middle,
            vec![para(HAlign::Center, vec![run(s, 30.0, Color::WHITE)])],
        );
        b.outline = Some(Outline {
            color: Color::rgb(200, 40, 40),
            width: 1.5,
        });
        node(NodeKind::Text(b))
    };
    let boxed = |t: Transform, s: &str| {
        node(NodeKind::Group(vec![
            fill(rect_path(rect(0.0, 0.0, 240.0, 60.0)), solid(60, 90, 160)),
            label(s),
        ]))
        .with_transform(t)
    };
    let s = scene(
        640.0,
        360.0,
        vec![
            background(640.0, 360.0, NAVY),
            boxed(
                rotation(-20.0, 120.0, 30.0).then(Transform::translate(40.0, 60.0)),
                "Rotated",
            ),
            boxed(
                Transform::scale(1.0, 1.6).then(Transform::translate(360.0, 30.0)),
                "Stretched",
            ),
            boxed(
                Transform::scale(-1.0, 1.0).then(Transform::translate(600.0, 260.0)),
                "Mirrored",
            ),
            boxed(
                Transform::scale(0.5, 0.5).then(Transform::translate(60.0, 280.0)),
                "Small",
            ),
        ],
    );
    check("text_rotated_and_scaled", &render(f, &s, 640, 360));
}

trait WithTransform {
    fn with_transform(self, t: Transform) -> Self;
}

impl WithTransform for Node {
    fn with_transform(self, transform: Transform) -> Self {
        at(transform, self)
    }
}

// Groups, transforms, opacity.

#[test]
fn groups_transforms_and_opacity() {
    let f = fixture_or_skip!();
    let pair = || {
        node(NodeKind::Group(vec![
            fill(circle(60.0, 60.0, 50.0), solid(255, 80, 80)),
            fill(circle(110.0, 60.0, 50.0), solid(80, 160, 255)),
        ]))
    };
    let nested = node(NodeKind::Group(vec![
        fill(rect_path(rect(0.0, 0.0, 100.0, 100.0)), solid(250, 200, 60)),
        at(
            rotation(45.0, 50.0, 50.0)
                .then(Transform::scale(0.5, 0.5))
                .then(Transform::translate(25.0, 25.0)),
            fill(rect_path(rect(0.0, 0.0, 100.0, 100.0)), solid(40, 160, 90)),
        ),
    ]));
    let hidden = Node {
        visible: false,
        ..fill(rect_path(rect(0.0, 0.0, 640.0, 360.0)), solid(255, 0, 255))
    };
    let s = scene(
        640.0,
        360.0,
        vec![
            background(640.0, 360.0, NAVY),
            // Opaque: the overlap is blue.
            at(Transform::translate(20.0, 20.0), pair()),
            // Group opacity: the circles fade as one, no darker overlap.
            faded(0.5, at(Transform::translate(220.0, 20.0), pair())),
            // Per-shape opacity: the overlap shows both.
            at(
                Transform::translate(420.0, 20.0),
                node(NodeKind::Group(vec![
                    faded(0.5, fill(circle(60.0, 60.0, 50.0), solid(255, 80, 80))),
                    faded(0.5, fill(circle(110.0, 60.0, 50.0), solid(80, 160, 255))),
                ])),
            ),
            at(
                rotation(15.0, 50.0, 50.0).then(Transform::translate(60.0, 200.0)),
                nested,
            ),
            // Nested opacity multiplies: 0.5 × 0.5.
            faded(
                0.5,
                at(
                    Transform::translate(260.0, 200.0),
                    node(NodeKind::Group(vec![faded(
                        0.5,
                        fill(
                            rect_path(rect(0.0, 0.0, 120.0, 120.0)),
                            solid(255, 255, 255),
                        ),
                    )])),
                ),
            ),
            at(
                Transform {
                    a: 1.0,
                    b: 0.0,
                    c: 0.5,
                    d: 1.0,
                    e: 430.0,
                    f: 200.0,
                },
                fill(
                    rect_path(rect(0.0, 0.0, 120.0, 120.0)),
                    solid(200, 120, 255),
                ),
            ),
            hidden,
        ],
    );
    check("groups_transforms_and_opacity", &render(f, &s, 640, 360));
}

#[test]
fn transparent_where_nothing_draws() {
    let f = fixture_or_skip!();
    let s = scene(
        100.0,
        100.0,
        vec![faded(
            0.5,
            fill(rect_path(rect(0.0, 0.0, 50.0, 100.0)), solid(255, 0, 0)),
        )],
    );
    let image = render(f, &s, 100, 100);
    let px = |x: usize, y: usize| &image.pixels[(y * 100 + x) * 4..][..4];
    assert_eq!(px(75, 50), [0, 0, 0, 0]);
    let left = px(25, 50);
    assert!(left[0] >= 250 && left[1] == 0 && left[2] == 0, "{left:?}");
    assert!((left[3] as i32 - 128).abs() <= 1, "{left:?}");
}

#[test]
fn scene_is_stretched_to_the_image() {
    let f = fixture_or_skip!();
    let s = scene(
        200.0,
        100.0,
        vec![fill(
            rect_path(rect(100.0, 0.0, 100.0, 100.0)),
            solid(0, 255, 0),
        )],
    );
    let image = render(f, &s, 40, 40);
    let px = |x: usize, y: usize| image.pixels[(y * 40 + x) * 4 + 1];
    assert_eq!((px(10, 20), px(30, 20)), (0, 255));
}

#[test]
fn bad_sizes_are_errors() {
    let f = fixture_or_skip!();
    let s = scene(10.0, 10.0, vec![]);
    assert!(matches!(
        f.renderer.render_offscreen(&s, 0, 10),
        Err(RenderError::Device(_))
    ));
    assert!(matches!(
        f.renderer.render_offscreen(&s, 10, 1 << 20),
        Err(RenderError::Device(_))
    ));
}

// Calibration pattern (TS-1).

mod calibration {
    use super::*;
    use lampstand_core::safe_area::SafeArea;
    use lampstand_render::calibrate::{self, CORNERS};
    use lampstand_render::output::overlay::{calibration_scene, safe_area_scene};
    use lampstand_render::{CalibrationPattern, Edge};

    const AREA: SafeArea = SafeArea {
        top: 0.05,
        right: 0.06,
        bottom: 0.08,
        left: 0.04,
    };

    fn pattern(active: Option<Edge>) -> CalibrationPattern {
        CalibrationPattern {
            current: AREA,
            max_percent: 15,
            active,
            label: "Main".into(),
        }
    }

    fn px(image: &Image, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * image.width + x) * 4) as usize;
        image.pixels[i..i + 4].try_into().unwrap()
    }

    fn rgba(c: Color) -> [u8; 4] {
        [c.r, c.g, c.b, 255]
    }

    /// Pixels in the box that are brighter than `min` in the red channel.
    fn bright(image: &Image, x: Range<u32>, y: Range<u32>, min: u8) -> usize {
        y.flat_map(|y| x.clone().map(move |x| (x, y)))
            .filter(|&(x, y)| px(image, x, y)[0] > min)
            .count()
    }

    use std::ops::Range;

    /// Checks by pixel, on outputs whose 1% is a whole number of pixels
    /// (so a ring's position is exact).
    fn check_pattern(image: &Image, active: Option<Edge>) {
        let (w, h) = (image.width, image.height);
        let (fw, fh) = (w as f32, h as f32);

        // A different colour in each corner, on the arm along the side.
        let (arm, i) = (fh * 0.04, 5);
        let corner_px = [
            (i, arm as u32),
            (w - 1 - i, arm as u32),
            (w - 1 - i, h - 1 - arm as u32),
            (i, h - 1 - arm as u32),
        ];
        for ((x, y), color) in corner_px.into_iter().zip(CORNERS) {
            assert_eq!(px(image, x, y), rgba(color), "corner at {x},{y}");
        }

        // The edge ring is on the very edge; ring 1 (dim) and ring 5 and
        // ring 10 (bright, alternating colours) are where they say, at the
        // left edge at a height clear of any number.
        let y = (fh * 0.45) as u32;
        assert_eq!(px(image, w / 2 + 100, 1), rgba(calibrate::EDGE));
        assert_eq!(px(image, w / 100, y), rgba(calibrate::ring_color(1)));
        assert_eq!(px(image, w * 5 / 100, y), rgba(calibrate::RING_B));
        assert_eq!(px(image, w * 10 / 100, y), rgba(calibrate::RING_A));
        // ... and on the top edge, 1% of the height down, at a clear x.
        let x = (fw * 0.625) as u32;
        assert_eq!(px(image, x, h / 100), rgba(calibrate::ring_color(1)));
        assert_eq!(px(image, x, h * 10 / 100), rgba(calibrate::RING_A));
        // Empty space is black.
        assert_eq!(
            px(image, w * 3 / 10 + 20, h * 3 / 10 + 20),
            rgba(Color::BLACK)
        );
        // The 10% grid.
        assert_eq!(
            px(image, w * 3 / 10 + 40, h * 3 / 10 - 1),
            rgba(calibrate::GRID)
        );

        // The safe area, in cyan: left at 4%, right at 6%, top at 5%.
        let left = (fw * AREA.left) as u32;
        let right = (fw * (1.0 - AREA.right)) as u32;
        let top = (fh * AREA.top) as u32;
        let cyan = rgba(calibrate::SAFE);
        assert_eq!(px(image, right - 2, y), cyan, "right edge");
        if active != Some(Edge::Top) {
            assert_eq!(px(image, x, top + 2), cyan, "top edge");
        }
        match active {
            // The edge being adjusted is white.
            Some(Edge::Left) => {
                assert_eq!(px(image, left + 8, y), rgba(calibrate::ACTIVE));
                assert_eq!(px(image, right - 2, y), cyan);
            }
            _ => assert_eq!(px(image, left + 2, y), cyan, "left edge"),
        }

        // Numbers are drawn: ring 7's small number, just inside ring 7,
        // in the second column of the top edge, and ring 5's large one just inside ring 5.
        let (cx, band) = ((fw * 0.345) as u32, (h * 7 / 100 + 1)..(h * 9 / 100));
        assert!(bright(image, cx - 12..cx + 12, band, 150) > 30, "small 7");
        let big = (h * 5 / 100 + 3)..(h * 5 / 100 + 3 + h / 30);
        let at = (fw * 0.14) as u32;
        assert!(bright(image, at - 20..at + 20, big, 200) > 40, "large 5");
        // The corner names, in their colours, at 20% and 80%.
        for ((fx, fy), color) in [(0.2, 0.2), (0.8, 0.2), (0.8, 0.8), (0.2, 0.8)]
            .into_iter()
            .zip(CORNERS)
        {
            let (cx, cy) = ((fw * fx) as u32, (fh * fy) as u32);
            let n = (cy - 30..cy + 30)
                .flat_map(|y| (cx - 40..cx + 40).map(move |x| (x, y)))
                .filter(|&(x, y)| px(image, x, y) == rgba(color))
                .count();
            assert!(n > 30, "corner name at {cx},{cy}: {n} pixels");
        }
        // The caption under the centre cross.
        let below = (h / 2 + h / 14)..(h / 2 + h / 5);
        assert!(
            bright(image, w / 2 - 150..w / 2 + 150, below, 200) > 200,
            "caption"
        );
    }

    #[test]
    fn calibration_16x9() {
        let f = fixture_or_skip!();
        let scene = calibration_scene(&pattern(Some(Edge::Left)), 1600.0, 900.0);
        let image = render(f, &scene, 1600, 900);
        check_pattern(&image, Some(Edge::Left));
        check("calibration_16x9", &image);
    }

    #[test]
    fn calibration_4x3() {
        let f = fixture_or_skip!();
        let scene = calibration_scene(&pattern(None), 1200.0, 900.0);
        let image = render(f, &scene, 1200, 900);
        check("calibration_4x3", &image);
        check_pattern(&image, None);
    }

    #[test]
    fn calibration_32x9() {
        let f = fixture_or_skip!();
        let scene = calibration_scene(&pattern(Some(Edge::Top)), 3200.0, 900.0);
        let image = render(f, &scene, 3200, 900);
        check_pattern(&image, Some(Edge::Top));
        check("calibration_32x9", &image);
    }

    /// Rows of the top-edge number for ring 3 (a single bold white digit,
    /// 2% of the height tall) that have white pixels in them: its ink
    /// height. Rings 4 and 5 cross the box but are dim grey and yellow.
    fn ink_rows(image: &Image) -> usize {
        let (w, h) = (image.width, image.height);
        let cx = (w as f32 * 0.30) as u32;
        (h * 3 / 100..h * 11 / 200)
            .filter(|&y| (cx - 12..cx + 12).any(|x| px(image, x, y)[..3].iter().all(|&c| c > 200)))
            .count()
    }

    #[test]
    fn calibration_720p() {
        let f = fixture_or_skip!();
        let scene = calibration_scene(&pattern(Some(Edge::Bottom)), 1280.0, 720.0);
        let image = render(f, &scene, 1280, 720);
        // The top and bottom numbers, the hard case at 1% steps, are
        // tall enough to read: a digit's ink is about 0.7 of its size.
        assert!(
            ink_rows(&image) >= 9,
            "ring 3 is {} px tall",
            ink_rows(&image)
        );
        check("calibration_720p", &image);
    }

    #[test]
    fn top_numbers_are_legible_on_xga_and_1080p() {
        let f = fixture_or_skip!();
        for (w, h) in [(1024, 768), (1920, 1080)] {
            let scene = calibration_scene(&pattern(None), w as f32, h as f32);
            let image = render(f, &scene, w, h);
            assert!(
                ink_rows(&image) >= 9,
                "{w}x{h}: ring 3 is {} px tall",
                ink_rows(&image)
            );
        }
    }

    #[test]
    fn the_edge_being_adjusted_is_the_one_that_changes() {
        let f = fixture_or_skip!();
        let render_with = |active| {
            render(
                f,
                &calibration_scene(&pattern(active), 1600.0, 900.0),
                1600,
                900,
            )
        };
        let none = render_with(None);
        for edge in Edge::ALL {
            let image = render_with(Some(edge));
            // The highlight is on the safe-area edge, clear of the labels.
            let (x, y) = match edge {
                Edge::Top => (1000, 45 + 8),
                Edge::Bottom => (1000, 900 - 72 - 8),
                Edge::Left => (64 + 8, 405),
                Edge::Right => (1504 - 8, 405),
            };
            assert_eq!(px(&image, x, y), rgba(calibrate::ACTIVE), "{edge:?}");
            assert_ne!(px(&none, x, y), rgba(calibrate::ACTIVE), "{edge:?} without");
        }
    }

    #[test]
    fn safe_area_overlay_for_previews() {
        let f = fixture_or_skip!();
        let scene = safe_area_scene(&AREA, 1600.0, 900.0);
        let image = render(f, &scene, 1600, 900);
        // Transparent but for a dashed outline at the margins.
        assert_eq!(px(&image, 800, 450)[3], 0);
        let dashes = (0..900)
            .filter(|&y| px(&image, 64, y)[3] > 128 || px(&image, 65, y)[3] > 128)
            .count();
        assert!(dashes > 100 && dashes < 700, "dashed, not solid: {dashes}");
        check("safe_area_overlay", &image);
    }
}
