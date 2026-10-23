//! Serves fixed scenes to a browser source and writes what `render_offscreen`
//! draws for each, so the Playwright test (`web/tests/obs.spec.ts`) can
//! compare the page with the GPU renderer.
//!
//! cargo run -p lampstand-server --example obs_fixture -- <dir>
//!
//! Writes `<dir>/<scene>.png` for every scene, prints
//! `READY <port> <output> <key>`, then reads lines from stdin: `show <scene>`
//! publishes that scene to the output and prints `SHOWN <scene>`. Prints
//! `SKIP <reason>` and exits when there is no GPU or software adapter.
//!
//! Scenes: `lyrics` (the compositor's stream lower third), `plate` (a
//! hand-built lower third with a gradient plate, bullets, mixed fonts and
//! shadows), `deck` (a slide, which the page gets as a WebP frame), `empty`.

use std::io::BufRead;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use lampstand_core::geom::{Color, Point, Rect, Transform};
use lampstand_core::id::{BlobHash, Id};
use lampstand_core::output::{BackgroundMode, OutputConfig, OutputId, OutputRole, OutputTarget};
use lampstand_core::program::{Content, Overlay, Program, SlideText};
use lampstand_core::safe_area::SafeArea;
use lampstand_core::scene::*;
use lampstand_core::song::SongId;
use lampstand_core::theme::{Background, TextStyle, Theme};
use lampstand_layout::{compose_at, Fonts, OutputSize};
use lampstand_render::{BlobSource, RenderError, Renderer, RendererOptions};
use lampstand_server::{Backend, SceneHub, Server};

struct Blobs;
impl BlobSource for Blobs {
    fn path(&self, _: &BlobHash) -> Option<PathBuf> {
        None
    }
}

struct App {
    hub: SceneHub,
    renderer: Arc<Renderer>,
}

impl Backend for App {
    fn output_key_valid(&self, output: OutputId, key: &str) -> bool {
        self.hub.key_valid(output, key)
    }
    fn watch_scene(&self, output: OutputId) -> Option<std::sync::mpsc::Receiver<Arc<Scene>>> {
        self.hub.watch(output)
    }
    fn blob_path(&self, _: &BlobHash) -> Option<PathBuf> {
        None
    }
    fn renderer(&self) -> Option<Arc<Renderer>> {
        Some(self.renderer.clone())
    }
}

fn node(id: NodeId, kind: NodeKind) -> Node {
    Node {
        id,
        transform: Transform::IDENTITY,
        opacity: 1.0,
        visible: true,
        kind,
    }
}

fn rounded(r: Rect, radius: f32) -> Vec<PathCommand> {
    let (x0, y0, x1, y1) = (r.x, r.y, r.x + r.width, r.y + r.height);
    let p = |x, y| Point { x, y };
    let k = radius * 0.552_284_8;
    vec![
        PathCommand::MoveTo(p(x0 + radius, y0)),
        PathCommand::LineTo(p(x1 - radius, y0)),
        PathCommand::CubicTo(
            p(x1 - radius + k, y0),
            p(x1, y0 + radius - k),
            p(x1, y0 + radius),
        ),
        PathCommand::LineTo(p(x1, y1 - radius)),
        PathCommand::CubicTo(
            p(x1, y1 - radius + k),
            p(x1 - radius + k, y1),
            p(x1 - radius, y1),
        ),
        PathCommand::LineTo(p(x0 + radius, y1)),
        PathCommand::CubicTo(
            p(x0 + radius - k, y1),
            p(x0, y1 - radius + k),
            p(x0, y1 - radius),
        ),
        PathCommand::LineTo(p(x0, y0 + radius)),
        PathCommand::CubicTo(
            p(x0, y0 + radius - k),
            p(x0 + radius - k, y0),
            p(x0 + radius, y0),
        ),
        PathCommand::Close,
    ]
}

fn run(text: &str, family: &str, size: f32, weight: u16, italic: bool, color: Color) -> TextRun {
    TextRun {
        text: text.into(),
        font_family: family.into(),
        size,
        weight,
        italic,
        underline: false,
        strike: false,
        color,
    }
}

fn lyrics(fonts: &mut Fonts) -> Scene {
    let style = TextStyle {
        font_family: "Noto Sans".into(),
        size: 64.0,
        min_size: 32.0,
        weight: 700,
        italic: false,
        color: Color::WHITE,
        align: HAlign::Center,
        valign: VAlign::Middle,
        line_spacing: 1.2,
        uppercase: false,
        shadow: Some(Shadow {
            color: Color {
                r: 0,
                g: 0,
                b: 0,
                a: 200,
            },
            offset: Point { x: 3.0, y: 4.0 },
            blur: 8.0,
        }),
        outline: Some(Outline {
            color: Color::BLACK,
            width: 2.0,
        }),
    };
    let theme = Theme {
        id: Id::generate(),
        name: "stream".into(),
        text: style,
        background: Background::None,
        text_area: Rect {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        },
        max_lines: 4,
        transition: Transition::CUT,
        show_copyright: true,
    };
    let output = OutputConfig {
        id: Id::generate(),
        name: "stream".into(),
        role: OutputRole::Stream,
        target: OutputTarget::Window {
            width: 1920,
            height: 1080,
        },
        safe_area: SafeArea {
            top: 0.05,
            right: 0.06,
            bottom: 0.08,
            left: 0.04,
        },
        background: BackgroundMode::Bleed,
        theme: None,
        idle: None,
        enabled: true,
    };
    let program = Program {
        content: Some(Content::Lyrics {
            song: SongId::generate(),
            slide: SlideText {
                label: Some("V1".into()),
                lines: [
                    "Amazing grace how sweet the sound",
                    "That saved a wretch like me",
                ]
                .map(str::to_string)
                .to_vec(),
                footer: Some("Public domain".into()),
            },
        }),
        overlay: Overlay::None,
        ..Program::default()
    };
    let size = OutputSize {
        width: 1920.0,
        height: 1080.0,
    };
    compose_at(
        &program,
        &output,
        size,
        &theme,
        fonts,
        lampstand_core::id::Timestamp(1_800_000_000_000),
        0,
    )
    .scene
}

fn plate() -> Scene {
    let ink = Color::rgb(250, 250, 245);
    let gold = Color::rgb(255, 200, 80);
    let plate_rect = Rect {
        x: 120.0,
        y: 700.0,
        width: 1200.0,
        height: 260.0,
    };
    let stop = |offset, a| GradientStop {
        offset,
        color: Color {
            r: 20,
            g: 40,
            b: 90,
            a,
        },
    };
    let plate = node(
        1 << 24,
        NodeKind::Path(PathNode {
            commands: rounded(plate_rect, 28.0),
            fill: Some(Paint::Linear {
                start: Point { x: 120.0, y: 700.0 },
                end: Point {
                    x: 1320.0,
                    y: 700.0,
                },
                stops: vec![stop(0.0, 255), stop(1.0, 40)],
            }),
            stroke: Some(Stroke {
                paint: Paint::Solid(gold),
                width: 4.0,
                dash: Some(vec![24.0, 12.0]),
            }),
            shadow: Some(Shadow {
                color: Color {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: 160,
                },
                offset: Point { x: 6.0, y: 8.0 },
                blur: 20.0,
            }),
        }),
    );
    let para = |bullet: Option<&str>, runs: Vec<TextRun>| Paragraph {
        align: HAlign::Left,
        line_spacing: 1.25,
        space_before: 4.0,
        space_after: 10.0,
        indent: 48.0,
        bullet: bullet.map(str::to_string),
        runs,
    };
    let text = node(
        (1 << 24) + 1,
        NodeKind::Text(TextBlock {
            rect: Rect {
                x: 170.0,
                y: 720.0,
                width: 1100.0,
                height: 220.0,
            },
            valign: VAlign::Middle,
            paragraphs: vec![
                para(
                    Some("•"),
                    vec![
                        run("Welcome ", "Calibri", 54.0, 700, false, gold),
                        run("to church", "Cambria", 54.0, 400, true, ink),
                    ],
                ),
                para(
                    Some("2."),
                    vec![
                        run(
                            "Please silence your phones ",
                            "Arial",
                            40.0,
                            400,
                            false,
                            ink,
                        ),
                        TextRun {
                            underline: true,
                            ..run(
                                "and welcome the people beside you",
                                "Times New Roman",
                                40.0,
                                400,
                                false,
                                ink,
                            )
                        },
                    ],
                ),
            ],
            shadow: Some(Shadow {
                color: Color::BLACK,
                offset: Point { x: 2.0, y: 2.0 },
                blur: 4.0,
            }),
            outline: None,
        }),
    );
    let mut group = node((1 << 24) + 2, NodeKind::Group(vec![plate, text]));
    group.opacity = 0.85;
    group.transform = Transform::translate(40.0, -20.0);
    Scene {
        width: 1920.0,
        height: 1080.0,
        nodes: vec![group],
    }
}

fn deck() -> Scene {
    let rect = |id, x, y, color| {
        node(
            id,
            NodeKind::Path(PathNode {
                commands: rounded(
                    Rect {
                        x,
                        y,
                        width: 300.0,
                        height: 200.0,
                    },
                    30.0,
                ),
                fill: Some(Paint::Solid(color)),
                stroke: None,
                shadow: None,
            }),
        )
    };
    let mut group = node(
        1 << 24,
        NodeKind::Group(vec![
            rect(1, 50.0, 100.0, Color::rgb(200, 30, 30)),
            rect(2, 500.0, 200.0, Color::rgb(30, 30, 200)),
        ]),
    );
    group.transform = Transform::scale(2.0, 2.0);
    Scene {
        width: 1920.0,
        height: 1080.0,
        nodes: vec![group],
    }
}

fn save(dir: &Path, name: &str, image: &lampstand_render::Image) {
    image::RgbaImage::from_raw(image.width, image.height, image.pixels.clone())
        .expect("image size")
        .save(dir.join(format!("{name}.png")))
        .expect("write png");
}

#[tokio::main]
async fn main() {
    let dir = PathBuf::from(std::env::args().nth(1).expect("usage: obs_fixture <dir>"));
    std::fs::create_dir_all(&dir).unwrap();
    let options = RendererOptions {
        system_fonts: false,
        ..RendererOptions::default()
    };
    let renderer = match Renderer::with_options(Arc::new(Blobs), options) {
        Ok(r) => Arc::new(r),
        Err(RenderError::NoAdapter) => {
            println!("SKIP no GPU or software adapter");
            return;
        }
        Err(e) => panic!("renderer: {e:?}"),
    };
    let mut fonts = Fonts::bundled_only();
    let scenes: Vec<(&str, Arc<Scene>)> = vec![
        ("lyrics", Arc::new(lyrics(&mut fonts))),
        ("plate", Arc::new(plate())),
        ("deck", Arc::new(deck())),
        (
            "empty",
            Arc::new(Scene {
                width: 1920.0,
                height: 1080.0,
                nodes: Vec::new(),
            }),
        ),
    ];
    for (name, scene) in &scenes {
        let image = renderer
            .render_offscreen(scene, scene.width as u32, scene.height as u32)
            .expect("render");
        save(&dir, name, &image);
        std::fs::write(
            dir.join(format!("{name}.json")),
            serde_json::to_vec(&**scene).unwrap(),
        )
        .unwrap();
    }

    let app = Arc::new(App {
        hub: SceneHub::new(),
        renderer,
    });
    let output = OutputId::generate();
    let key = app.hub.add_output(output);
    let server = Server::start(SocketAddr::from(([127, 0, 0, 1], 0)), app.clone())
        .await
        .expect("server");
    println!("READY {} {output} {key}", server.local_addr().port());

    for line in std::io::stdin().lock().lines() {
        let line = line.unwrap();
        if let Some(name) = line.strip_prefix("show ") {
            match scenes.iter().find(|(n, _)| *n == name.trim()) {
                Some((n, scene)) => {
                    app.hub.publish(output, scene.clone());
                    println!("SHOWN {n}");
                }
                None => println!("UNKNOWN {name}"),
            }
        }
    }
}
