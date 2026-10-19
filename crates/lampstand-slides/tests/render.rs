//! T29 acceptance, visual half: every visible slide of the synthetic decks,
//! drawn by the wgpu renderer (T24) at one pixel per point, against the
//! reference render in `tests/decks/ref/`.
//!
//! The references are LibreOffice's, not PowerPoint's (see
//! `tests/decks/reference.sh`), so the bar is "visibly the same slide", not
//! pixel equality: text sets in different substitute fonts and wraps a
//! little differently. Each slide must stay within `MAX_RMSE` (normalised
//! RMS difference over RGB); a broken mapping (a missing background, a
//! shape in the wrong place, the wrong colours) goes well past it.
//!
//! Every run writes `target/slides-decks/<deck>-NN.png`: ours on the left,
//! the reference on the right, to look at. Where no GPU adapter exists the
//! test prints that it skipped.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use lampstand_core::id::BlobHash;
use lampstand_render::{AdapterChoice, BlobSource, RenderError, Renderer, RendererOptions};
use lampstand_slides::import_pptx;

/// The largest per-slide difference allowed, from 0 (identical) to 1.
const MAX_RMSE: f64 = 0.16;

/// Slides where LibreOffice, not the parser, is wrong, with the bar they
/// are held to instead. Each was checked by eye.
const LIBREOFFICE_WRONG: &[(&str, usize, f64, &str)] = &[
    (
        "02-theme-colors-fonts",
        2,
        0.20,
        "LibreOffice wraps the Cambria title onto two lines in a box it fits on one",
    ),
    (
        "10-church-service",
        2,
        0.32,
        "LibreOffice ignores the saved 77.5% normAutofit scale and refits the psalm larger",
    ),
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
            eprintln!("{name} slide {}: rmse {score:.3}", i + 1);
            let bar = LIBREOFFICE_WRONG
                .iter()
                .find(|(deck, slide, _, _)| *deck == name && *slide == i + 1)
                .map_or(MAX_RMSE, |(_, _, bar, _)| *bar);
            if score > bar {
                failures.push(format!("{name} slide {}: {score:.3}", i + 1));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "slides differ from their references by more than allowed (see target/slides-decks/):\n{}",
        failures.join("\n")
    );
}
