//! Decks and media from disk or a link: `AddDeckFile`, `AddDeckFromLink`,
//! `AddMediaFile` and `ListMedia`, and turning a deck into the slides the
//! live engine shows.
//!
//! A deck is parsed once when it is added, so a file that is not a deck
//! (or one the T87 budget refuses) never reaches a service; the bytes go
//! into the blob store and a `Deck` document points at them. Parsing is
//! derived data: each device parses a deck again when a service that uses
//! it loads, and keeps the result while that service is open.
//!
//! Like `import`, these take the library, not the app, so the shell runs
//! them off the `App` lock: a long video copy or a download never holds up
//! live control.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use lampstand_api::MediaFile;
use lampstand_connectors::links::{self, Link};
use lampstand_connectors::{FetchError, Fetched};
use lampstand_core::deck::{Deck, DeckId, DeckSource, Version};
use lampstand_core::id::{BlobHash, Timestamp};
use lampstand_core::scene::{Animation, Scene, Transition};
use lampstand_core::service::ItemKind;
use lampstand_core::{FontLookup, FontMatch};
use lampstand_library::Library;
use lampstand_slides::{import_pptx, ImportError};

/// A deck as the live engine needs it: the slides to show, in order, with
/// hidden ones left out.
#[derive(Debug)]
pub struct ParsedSlides {
    pub slides: Vec<Slide>,
}

#[derive(Debug)]
pub struct Slide {
    /// 0-based, in the source deck (hidden slides count), for "Slide 3".
    pub index: usize,
    /// The slide after each build step, `scenes[0]` as it stands once
    /// anything that plays as it arrives has played: what outputs are
    /// composed from, and thumbnails. One more than `builds`.
    pub scenes: Vec<Arc<Scene>>,
    /// The slide as it arrives, before `entry` plays (`scenes[0]` when
    /// nothing plays by itself).
    pub arrival: Arc<Scene>,
    /// Animations that play by themselves as the slide arrives.
    pub entry: Vec<Animation>,
    /// One list of animations per click.
    pub builds: Vec<Vec<Animation>>,
    /// How the slide arrives on the outputs.
    pub transition: Transition,
    pub notes: String,
}

impl ParsedSlides {
    /// The slide with this index in the source deck.
    pub fn slide(&self, index: usize) -> Option<&Slide> {
        self.slides.iter().find(|s| s.index == index)
    }
}

impl Slide {
    /// The slide after `step` build steps; past the last, the last.
    pub fn scene_at(&self, step: usize) -> Arc<Scene> {
        self.scenes[step.min(self.scenes.len() - 1)].clone()
    }
}

/// Checks that a deck parses: every font counts as present, so the check
/// doesn't load a font set. The real fonts are used when it loads.
struct AnyFont;

impl FontLookup for AnyFont {
    fn lookup(&self, _: &str) -> FontMatch {
        FontMatch::Exact
    }
}

/// Why a deck could not be added or shown, for the operator.
pub fn describe(e: &ImportError) -> String {
    match e {
        ImportError::NotAPresentation => "not a PowerPoint (.pptx) file".into(),
        ImportError::Encrypted => "it is password-protected; save a copy without a password".into(),
        ImportError::Corrupt(why) => format!("it could not be read ({why})"),
        ImportError::TooLarge { .. } => "it is too large to open safely; \
             link large video or add it as a media item instead of embedding it"
            .into(),
    }
}

/// "Easter Sunday.pptx" → "Easter Sunday".
fn title_of(name: &str) -> String {
    let stem = Path::new(name)
        .file_stem()
        .map(|s| s.to_string_lossy().trim().to_string())
        .unwrap_or_default();
    if stem.is_empty() {
        "Presentation".into()
    } else {
        stem
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn now() -> Timestamp {
    Timestamp(chrono::Utc::now().timestamp_millis())
}

/// Checks `bytes` parse as a deck, stores them and adds the `Deck`.
pub fn add_deck_bytes(
    library: &Library,
    name: &str,
    bytes: &[u8],
    source: DeckSource,
    source_version: Option<String>,
) -> Result<Deck, String> {
    import_pptx(bytes, &AnyFont).map_err(|e| format!("{name}: {}", describe(&e)))?;
    let blob = library
        .repo()
        .blobs()
        .put(bytes)
        .map_err(|e| e.to_string())?;
    // The same file added again (picked twice, or the same link) is the
    // deck already in the library, not a second copy.
    let existing = library.decks().map_err(|e| e.to_string())?;
    if let Some(deck) = existing.into_iter().find(|d| d.current.blob == blob) {
        return Ok(deck);
    }
    let deck = Deck {
        id: DeckId::generate(),
        title: title_of(name),
        source,
        current: Version {
            blob,
            fetched_at: now(),
            source_version,
        },
        pending: None,
    };
    library.put_deck(&deck).map_err(|e| e.to_string())?;
    Ok(deck)
}

/// `LibraryRequest::AddDeckFile`.
pub fn add_deck_file(library: &Library, path: &str) -> Result<Deck, String> {
    let path = Path::new(path);
    let name = file_name(path);
    let bytes = std::fs::read(path).map_err(|e| format!("{name}: {e}"))?;
    add_deck_bytes(
        library,
        &name,
        &bytes,
        DeckSource::File { name: name.clone() },
        None,
    )
}

/// Where a pasted link is recorded as coming from.
fn link_source(link: &Link, url: &str) -> DeckSource {
    match link {
        Link::GoogleSlides { .. } => DeckSource::GoogleSlidesLink {
            url: url.trim().to_string(),
        },
        Link::GoogleDriveFile { id } => DeckSource::GoogleDrive {
            file_id: id.clone(),
            google_slides: false,
        },
        Link::OneDrive { url } => DeckSource::OneDriveLink { url: url.clone() },
    }
}

/// What a pasted link points at, or why it isn't a link we can fetch.
pub fn classify(url: &str) -> Result<Link, String> {
    links::classify(url).ok_or_else(|| {
        "that is not a Google Slides, Google Drive or OneDrive share link".to_string()
    })
}

/// `LibraryRequest::AddDeckFromLink`: downloads the file (async, in the
/// shell), then `finish` checks and stores it off the runtime.
pub async fn fetch_link(link: &Link) -> Result<Fetched, String> {
    match links::fetch(link, None).await {
        Ok(Some(fetched)) => Ok(fetched),
        // Only answered when a previous version is passed.
        Ok(None) => Err("the link returned nothing".into()),
        Err(e) => Err(match e {
            FetchError::Network(why) => format!("the download failed ({why})"),
            FetchError::Forbidden => {
                "the link is private: share it as \"anyone with the link\"".into()
            }
            FetchError::NotFound => "nothing is shared at that link".into(),
            FetchError::NotADeck(_) => "the link does not lead to a presentation".into(),
        }),
    }
}

/// Adds a deck downloaded from `url` (classified as `link`).
pub fn add_fetched(
    library: &Library,
    link: &Link,
    url: &str,
    fetched: Fetched,
) -> Result<Deck, String> {
    add_deck_bytes(
        library,
        &fetched.name,
        &fetched.bytes,
        link_source(link, url),
        fetched.version,
    )
}

/// `LibraryRequest::AddMediaFile`: streams the file into the blob store.
pub fn add_media_file(library: &Library, path: &str) -> Result<MediaFile, String> {
    let path = Path::new(path);
    let name = file_name(path);
    let blob = library
        .repo()
        .blobs()
        .import(path)
        .map_err(|e| format!("{name}: {e}"))?;
    Ok(MediaFile {
        blob,
        name,
        on_device: true,
    })
}

const VIDEO_EXTENSIONS: [&str; 5] = ["mp4", "mov", "m4v", "webm", "mkv"];

/// `LibraryRequest::MediaThumbnail`: a JPEG `data:` URL, `width` pixels wide,
/// of an image, or of the first frame of a video that is not blank.
pub fn media_thumbnail(
    library: &Library,
    blob: &BlobHash,
    name: &str,
    width: u32,
) -> Result<String, String> {
    use base64::Engine;
    let path = library
        .repo()
        .blobs()
        .path(blob)
        .ok_or("this file is not on this device yet")?;
    let is_video = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| VIDEO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()));
    let picture = if is_video {
        let frame = lampstand_media::poster_best(&path).map_err(|e| e.to_string())?;
        image::RgbaImage::from_raw(frame.width, frame.height, frame.rgba)
            .ok_or("the video's picture has the wrong size")?
            .into()
    } else {
        image::ImageReader::open(&path)
            .map_err(|e| e.to_string())?
            .with_guessed_format()
            .map_err(|e| e.to_string())?
            .decode()
            .map_err(|e| e.to_string())?
    };
    let width = width.clamp(16, 640);
    let height = (u64::from(picture.height()) * u64::from(width)
        / u64::from(picture.width().max(1)))
    .max(1) as u32;
    let small = picture
        .resize_exact(width, height, image::imageops::FilterType::Triangle)
        .to_rgb8();
    let mut jpeg = std::io::Cursor::new(Vec::new());
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 80)
        .encode_image(&small)
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(jpeg.into_inner())
    ))
}

/// `LibraryRequest::ListMedia`: every `Media` item in every service, once
/// per blob, by name.
pub fn list_media(library: &Library) -> Result<Vec<MediaFile>, String> {
    let mut seen: BTreeMap<BlobHash, String> = BTreeMap::new();
    for service in library.services().map_err(|e| e.to_string())? {
        for (_, item) in service.ordered() {
            if let ItemKind::Media { blob, name, .. } = &item.kind {
                seen.entry(*blob).or_insert_with(|| name.clone());
            }
        }
    }
    let blobs = library.repo().blobs();
    let mut media: Vec<MediaFile> = seen
        .into_iter()
        .map(|(blob, name)| MediaFile {
            on_device: blobs.path(&blob).is_some(),
            blob,
            name,
        })
        .collect();
    media.sort_by_key(|m| m.name.to_lowercase());
    Ok(media)
}

/// Parses a deck's current version with `fonts` and stores the pictures
/// and media its slides draw, so the renderer finds them by hash.
pub fn parse(
    library: &Library,
    deck: &Deck,
    fonts: &dyn FontLookup,
) -> Result<ParsedSlides, String> {
    let blobs = library.repo().blobs();
    let path = blobs
        .path(&deck.current.blob)
        .ok_or("not on this device yet")?;
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let parsed = import_pptx(&bytes, fonts).map_err(|e| describe(&e))?;
    drop(bytes);
    for (hash, part) in &parsed.media {
        if blobs.path(hash).is_none() {
            blobs.put(&part.bytes).map_err(|e| e.to_string())?;
        }
    }
    let slides = parsed
        .slides
        .into_iter()
        .enumerate()
        .filter(|(_, s)| !s.hidden)
        .map(|(index, s)| {
            let scenes: Vec<Arc<Scene>> = (0..=s.builds.len())
                .map(|step| Arc::new(s.scene_at_step(step)))
                .collect();
            let arrival = if s.entry.animations.is_empty() {
                scenes[0].clone()
            } else {
                Arc::new(s.scene)
            };
            Slide {
                index,
                scenes,
                arrival,
                entry: s.entry.animations,
                builds: s.builds.into_iter().map(|b| b.animations).collect(),
                transition: s.transition,
                notes: s.notes,
            }
        })
        .collect();
    Ok(ParsedSlides { slides })
}

/// A rendered slide as a PNG `data:` URL, for an `<img>` in the operator
/// view.
pub fn png_data_url(image: &lampstand_render::Image) -> Result<String, String> {
    use base64::Engine;
    let rgba = image::RgbaImage::from_raw(image.width, image.height, image.pixels.clone())
        .ok_or("the rendered image has the wrong size")?;
    let mut png = std::io::Cursor::new(Vec::new());
    rgba.write_to(&mut png, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png.into_inner())
    ))
}

/// The thumbnail size for a scene `width` pixels wide, keeping its shape.
pub fn thumbnail_size(scene: &Scene, width: u32) -> (u32, u32) {
    let width = width.clamp(16, 1920);
    let height = if scene.width > 0.0 {
        (width as f32 * scene.height / scene.width).round() as u32
    } else {
        width * 9 / 16
    };
    (width, height.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lampstand_core::service::Service;

    fn deck_path(name: &str) -> String {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/lampstand-slides/tests/decks")
            .join(format!("{name}.pptx"))
            .display()
            .to_string()
    }

    fn library() -> (tempfile::TempDir, Library) {
        let dir = tempfile::tempdir().unwrap();
        let key = lampstand_sync::team::DeviceKeypair::generate();
        let lib = Library::open(dir.path(), key).unwrap();
        (dir, lib)
    }

    #[test]
    fn a_pptx_becomes_a_deck_titled_by_its_file() {
        let (_dir, lib) = library();
        let deck = add_deck_file(&lib, &deck_path("10-church-service")).unwrap();
        assert_eq!(deck.title, "10-church-service");
        assert_eq!(
            deck.source,
            DeckSource::File {
                name: "10-church-service.pptx".into()
            }
        );
        assert_eq!(lib.decks().unwrap(), vec![deck.clone()]);
        let slides = parse(&lib, &deck, &AnyFont).unwrap();
        assert!(!slides.slides.is_empty());
        // Picked again: the same deck, not a copy.
        let again = add_deck_file(&lib, &deck_path("10-church-service")).unwrap();
        assert_eq!(again, deck);
        assert_eq!(lib.decks().unwrap().len(), 1);
    }

    #[test]
    fn pictures_in_a_deck_reach_the_blob_store() {
        let (_dir, lib) = library();
        let deck = add_deck_file(&lib, &deck_path("07-pictures-media")).unwrap();
        let bytes = std::fs::read(deck_path("07-pictures-media")).unwrap();
        let parsed = import_pptx(&bytes, &AnyFont).unwrap();
        assert!(!parsed.media.is_empty());
        parse(&lib, &deck, &AnyFont).unwrap();
        for hash in parsed.media.keys() {
            assert!(lib.repo().blobs().path(hash).is_some());
        }
    }

    #[test]
    fn a_file_that_is_not_a_deck_is_refused() {
        let (dir, lib) = library();
        let path = dir.path().join("notes.pptx");
        std::fs::write(&path, b"not a zip").unwrap();
        let e = add_deck_file(&lib, &path.display().to_string()).unwrap_err();
        assert!(e.starts_with("notes.pptx: "), "{e}");
        assert!(lib.decks().unwrap().is_empty());
    }

    #[test]
    fn media_is_listed_once_from_the_services_that_use_it() {
        let (dir, lib) = library();
        let path = dir.path().join("Welcome loop.mp4");
        std::fs::write(&path, b"video bytes").unwrap();
        let media = add_media_file(&lib, &path.display().to_string()).unwrap();
        assert_eq!(media.name, "Welcome loop.mp4");
        assert_eq!(media.blob, BlobHash::of(b"video bytes"));
        let missing = BlobHash::of(b"elsewhere");
        for _ in 0..2 {
            let mut service = Service::new("Sunday");
            service.insert(
                0,
                ItemKind::Media {
                    blob: media.blob,
                    name: media.name.clone(),
                    looped: true,
                },
            );
            service.insert(
                1,
                ItemKind::Media {
                    blob: missing,
                    name: "announcements.png".into(),
                    looped: false,
                },
            );
            lib.put_service(&service).unwrap();
        }
        let listed = list_media(&lib).unwrap();
        assert_eq!(
            listed,
            vec![
                MediaFile {
                    blob: missing,
                    name: "announcements.png".into(),
                    on_device: false,
                },
                media,
            ]
        );
    }

    #[test]
    fn a_thumbnail_keeps_the_slide_shape_and_encodes_as_png() {
        let scene = Scene {
            width: 960.0,
            height: 540.0,
            nodes: Vec::new(),
        };
        assert_eq!(thumbnail_size(&scene, 320), (320, 180));
        let image = lampstand_render::Image {
            width: 2,
            height: 1,
            pixels: vec![255, 0, 0, 255, 0, 0, 255, 255],
        };
        let url = png_data_url(&image).unwrap();
        assert!(
            url.starts_with("data:image/png;base64,iVBORw0KGgo"),
            "{url}"
        );
    }

    #[test]
    fn links_are_classified_with_their_source() {
        let url = "https://docs.google.com/presentation/d/abc123/edit";
        let link = classify(url).unwrap();
        assert_eq!(
            link_source(&link, url),
            DeckSource::GoogleSlidesLink { url: url.into() }
        );
        assert!(classify("https://example.com/talk.pptx").is_err());
        assert_eq!(title_of("Easter Sunday.pptx"), "Easter Sunday");
    }

    fn decode(url: &str) -> image::DynamicImage {
        use base64::Engine;
        let b64 = url
            .strip_prefix("data:image/jpeg;base64,")
            .expect("a jpeg data url");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .unwrap();
        image::load_from_memory(&bytes).unwrap()
    }

    #[test]
    fn an_image_gets_a_thumbnail_of_the_width_asked_for() {
        let (dir, lib) = library();
        let path = dir.path().join("photo.png");
        image::RgbaImage::from_fn(400, 200, |x, y| {
            image::Rgba([(x % 256) as u8, (y % 256) as u8, 90, 255])
        })
        .save(&path)
        .unwrap();
        let media = add_media_file(&lib, path.to_str().unwrap()).unwrap();
        let thumb = decode(&media_thumbnail(&lib, &media.blob, "photo.png", 100).unwrap());
        assert_eq!(
            (thumb.width(), thumb.height()),
            (100, 50),
            "the shape is kept"
        );
    }

    #[test]
    fn a_video_gets_a_thumbnail_from_a_frame_that_is_not_blank() {
        let (_dir, lib) = library();
        let clip = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/lampstand-media/tests/fixtures/clip-2s.mp4");
        let media = add_media_file(&lib, clip.to_str().unwrap()).unwrap();
        let thumb = decode(&media_thumbnail(&lib, &media.blob, "clip-2s.mp4", 80).unwrap());
        assert_eq!((thumb.width(), thumb.height()), (80, 45));
        // Not one flat colour.
        let rgb = thumb.to_rgb8();
        assert!(rgb.pixels().any(|p| p != rgb.get_pixel(0, 0)));
    }

    #[test]
    fn a_file_that_is_not_on_this_device_has_no_thumbnail() {
        let (_dir, lib) = library();
        let missing = BlobHash::of(b"never imported");
        let error = media_thumbnail(&lib, &missing, "x.mp4", 80).unwrap_err();
        assert!(error.contains("not on this device"), "{error}");
    }
}
