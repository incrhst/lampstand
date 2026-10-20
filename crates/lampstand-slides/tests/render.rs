//! T29 acceptance, visual half: every visible slide of the synthetic decks,
//! drawn by the wgpu renderer (T24) at one pixel per point, against the
//! reference render in `tests/decks/ref/`.
//!
//! The references are LibreOffice's, not PowerPoint's (see
//! `tests/decks/reference.sh`), so slides never match exactly: text sets in
//! different substitute fonts and wraps a little differently. Instead each
//! slide is pinned to the difference it had when it was checked by eye
//! (`BASELINE`), measured two ways: the normalised RMS difference over the
//! whole slide, which catches a wrong background or colour scheme, and the
//! worst 64 × 64 tile, which catches one missing or misplaced shape that
//! the whole-slide figure would average away. A slide that gets worse than
//! its baseline by more than `MARGIN` fails.
//!
//! After an intended change, look at the side-by-sides, then paste the
//! table the test prints over `BASELINE`.
//!
//! Every run writes `target/slides-decks/<deck>-NN.png`: ours on the left,
//! the reference on the right, to look at. Where no GPU adapter exists the
//! test prints that it skipped.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use lampstand_core::id::BlobHash;
use lampstand_render::{AdapterChoice, BlobSource, RenderError, Renderer, RendererOptions};
use lampstand_slides::import_pptx;

/// How much worse than its baseline a slide may get: (whole slide, worst
/// tile). Leaves room for rasterisation differences between GPUs.
const MARGIN: (f64, f64) = (0.02, 0.08);

/// Tile side for the local check, in pixels (points).
const TILE: u32 = 64;

/// (deck, slide, whole-slide RMSE, worst-tile RMSE), as checked by eye.
/// Notable differences, all LibreOffice's: it ignores a saved `normAutofit
/// fontScale` and refits text larger (03 slide 3, 10 slide 2), wraps the
/// Cambria title on 02 slide 2 and a `wrap="none"` box on 03 slide 3,
/// drops text shadows (10 slide 1) and does not bold table header text
/// (08, 10 slide 3).
// Measured scores; one happens to be close to 1/π.
#[allow(clippy::approx_constant)]
const BASELINE: &[(&str, usize, f64, f64)] = &[
    ("01-masters-placeholders", 1, 0.072, 0.328),
    ("01-masters-placeholders", 2, 0.101, 0.395),
    ("01-masters-placeholders", 3, 0.056, 0.266),
    ("01-masters-placeholders", 5, 0.059, 0.259),
    ("02-theme-colors-fonts", 1, 0.082, 0.397),
    ("02-theme-colors-fonts", 2, 0.168, 0.458),
    ("03-text", 1, 0.156, 0.410),
    ("03-text", 2, 0.118, 0.389),
    ("03-text", 3, 0.139, 0.380),
    ("04-preset-shapes", 1, 0.071, 0.313),
    ("04-preset-shapes", 2, 0.074, 0.341),
    ("04-preset-shapes", 3, 0.065, 0.318),
    ("05-groups-connectors", 1, 0.046, 0.236),
    ("05-groups-connectors", 2, 0.075, 0.288),
    ("06-fills-lines-shadows", 1, 0.065, 0.211),
    ("06-fills-lines-shadows", 2, 0.078, 0.293),
    ("07-pictures-media", 1, 0.062, 0.277),
    ("07-pictures-media", 2, 0.040, 0.267),
    ("08-tables", 1, 0.070, 0.300),
    ("09-backgrounds", 1, 0.059, 0.277),
    ("09-backgrounds", 2, 0.037, 0.152),
    ("09-backgrounds", 3, 0.049, 0.205),
    ("09-backgrounds", 4, 0.042, 0.153),
    ("10-church-service", 1, 0.092, 0.251),
    ("10-church-service", 2, 0.284, 0.494),
    ("10-church-service", 3, 0.079, 0.324),
    ("10-church-service", 4, 0.144, 0.419),
];

struct Blobs {
    dir: tempfile::TempDir,
}

impl BlobSource for Blobs {
    fn path(&self, blob: &BlobHash) -> Option<PathBuf> {
        let path = self.dir.path().join(blob.to_string());
        path.exists().then_some(path)
    }
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn rmse(a: &image::RgbImage, b: &image::RgbImage) -> f64 {
    let sum: f64 = a
        .as_raw()
        .iter()
        .zip(b.as_raw())
        .map(|(x, y)| {
            let d = (f64::from(*x) - f64::from(*y)) / 255.0;
            d * d
        })
        .sum();
    (sum / a.as_raw().len() as f64).sqrt()
}

/// The largest RMSE over `TILE` × `TILE` tiles (edge tiles may be smaller).
fn worst_tile(a: &image::RgbImage, b: &image::RgbImage) -> f64 {
    let (w, h) = a.dimensions();
    let mut worst = 0.0_f64;
    for ty in (0..h).step_by(TILE as usize) {
        for tx in (0..w).step_by(TILE as usize) {
            let (tw, th) = (TILE.min(w - tx), TILE.min(h - ty));
            let mut sum = 0.0;
            for y in ty..ty + th {
                for x in tx..tx + tw {
                    let (p, q) = (a.get_pixel(x, y), b.get_pixel(x, y));
                    for c in 0..3 {
                        let d = (f64::from(p[c]) - f64::from(q[c])) / 255.0;
                        sum += d * d;
                    }
                }
            }
            worst = worst.max((sum / f64::from(tw * th * 3)).sqrt());
        }
    }
    worst
}

/// Over white, as a slide is projected.
fn flatten(width: u32, height: u32, rgba: &[u8]) -> image::RgbImage {
    let mut out = image::RgbImage::new(width, height);
    for (px, src) in out.pixels_mut().zip(rgba.as_chunks::<4>().0) {
        let a = u32::from(src[3]);
        // The renderer's pixels are premultiplied.
        let over = |c: u8| (u32::from(c) + (255 - a)).min(255) as u8;
        *px = image::Rgb([over(src[0]), over(src[1]), over(src[2])]);
    }
    out
}

#[test]
fn decks_render_like_their_references() {
    let blobs = Arc::new(Blobs {
        dir: tempfile::tempdir().unwrap(),
    });
    let options = RendererOptions {
        adapter: AdapterChoice::PreferIntegrated,
        system_fonts: false,
    };
    let renderer = match Renderer::with_options(blobs.clone(), options) {
        Ok(r) => r,
        Err(RenderError::NoAdapter) => {
            eprintln!("no GPU adapter: skipped");
            return;
        }
        Err(e) => panic!("renderer: {e:?}"),
    };
    let out = root().join("../../target/slides-decks");
    std::fs::create_dir_all(&out).unwrap();
    let fonts = lampstand_layout::Fonts::bundled_only();
    let mut failures = Vec::new();
    let mut measured = Vec::new();
    let mut decks: Vec<PathBuf> = std::fs::read_dir(root().join("tests/decks"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "pptx"))
        .collect();
    decks.sort();
    assert_eq!(decks.len(), 10);
    for path in decks {
        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        let deck = import_pptx(&std::fs::read(&path).unwrap(), &fonts).unwrap();
        for (hash, part) in &deck.media {
            std::fs::write(blobs.dir.path().join(hash.to_string()), &part.bytes).unwrap();
        }
        for (i, slide) in deck.slides.iter().enumerate() {
            let reference = root().join(format!("tests/decks/ref/{name}/slide-{:02}.png", i + 1));
            if slide.hidden {
                assert!(!reference.exists(), "{name} slide {} is hidden", i + 1);
                continue;
            }
            let (w, h) = (deck.width.round() as u32, deck.height.round() as u32);
            let image = renderer.render_offscreen(&slide.scene, w, h).unwrap();
            let ours = flatten(image.width, image.height, &image.pixels);
            let theirs = image::open(&reference)
                .unwrap_or_else(|e| panic!("{}: {e}", reference.display()))
                .to_rgb8();
            let theirs =
                image::imageops::resize(&theirs, w, h, image::imageops::FilterType::Triangle);
            let score = rmse(&ours, &theirs);
            let mut both = image::RgbImage::new(w * 2, h);
            image::imageops::replace(&mut both, &ours, 0, 0);
            image::imageops::replace(&mut both, &theirs, i64::from(w), 0);
            both.save(out.join(format!("{name}-{:02}.png", i + 1)))
                .unwrap();
            let tile = worst_tile(&ours, &theirs);
            measured.push(format!(
                "    (\"{name}\", {}, {score:.3}, {tile:.3}),",
                i + 1
            ));
            match BASELINE
                .iter()
                .find(|(deck, slide, _, _)| *deck == name && *slide == i + 1)
            {
                Some((_, _, whole, worst)) => {
                    if score > whole + MARGIN.0 || tile > worst + MARGIN.1 {
                        failures.push(format!(
                            "{name} slide {}: rmse {score:.3} (baseline {whole:.3}), worst tile {tile:.3} (baseline {worst:.3})",
                            i + 1
                        ));
                    }
                }
                None => failures.push(format!("{name} slide {}: no baseline", i + 1)),
            }
        }
    }
    eprintln!("measured:\n{}", measured.join("\n"));

    // The bar must catch one missing shape: drop the logo (a 158 pt
    // picture) from the pictures slide and check it fails.
    let path = root().join("tests/decks/07-pictures-media.pptx");
    let deck = import_pptx(&std::fs::read(&path).unwrap(), &fonts).unwrap();
    let mut scene = deck.slides[0].scene.clone();
    let logo = scene
        .nodes
        .iter()
        .position(|n| matches!(&n.kind, lampstand_core::scene::NodeKind::Image(i) if i.rect.width < 200.0))
        .expect("the logo");
    scene.nodes.remove(logo);
    let (w, h) = (deck.width.round() as u32, deck.height.round() as u32);
    let image = renderer.render_offscreen(&scene, w, h).unwrap();
    let ours = flatten(image.width, image.height, &image.pixels);
    let theirs = image::open(root().join("tests/decks/ref/07-pictures-media/slide-01.png"))
        .unwrap()
        .to_rgb8();
    let theirs = image::imageops::resize(&theirs, w, h, image::imageops::FilterType::Triangle);
    let (_, _, whole, worst) = BASELINE
        .iter()
        .find(|(d, s, _, _)| *d == "07-pictures-media" && *s == 1)
        .unwrap();
    let (score, tile) = (rmse(&ours, &theirs), worst_tile(&ours, &theirs));
    eprintln!("without the logo: rmse {score:.3}, worst tile {tile:.3}");
    assert!(
        score > whole + MARGIN.0 || tile > worst + MARGIN.1,
        "a slide missing a picture passes: rmse {score:.3}, worst tile {tile:.3}"
    );
    assert!(
        failures.is_empty(),
        "slides differ from their baselines (see target/slides-decks/):\n{}",
        failures.join("\n")
    );
}
