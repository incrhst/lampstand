//! Brand files: a church's look in one file, to set up once and share with
//! another computer or another campus.
//!
//! Format: a zip with the `.lampstand-brand` extension.
//!
//! ```text
//! manifest.json   format version, app version
//! brand.json      the brand, its themes (not an untouched Starter theme),
//!                 which one was made from the brand, and the idle screen
//! blobs/<hash>    the logo and every picture or video the above use
//! ```
//!
//! Fonts travel by family name only. A video over [`MAX_VIDEO_BYTES`], or
//! a file not on this device, is left out with a message, and what used it
//! falls back (a theme to no background, the idle screen to plain). A deck
//! slide is not bundled either.
//!
//! Opening a file never overwrites by ID: every theme gets a new ID. With
//! [`ImportMode::AddAlongside`] they are added next to the existing ones;
//! with [`ImportMode::Replace`] a theme replaces the one of the same name
//! (never the Starter theme) and is added if there is none.

use std::collections::HashSet;
use std::io::{Read, Seek, Write};

use lampstand_core::brand::Brand;
use lampstand_core::id::BlobHash;
use lampstand_core::output::IdleScreen;
use lampstand_core::theme::{Background, Theme, ThemeId};
use serde::{Deserialize, Serialize};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::{starter_theme, Library};

pub const FORMAT_VERSION: u32 = 1;

/// A longer video is not put in the file.
pub const MAX_VIDEO_BYTES: u64 = 150 * 1024 * 1024;

#[derive(Debug)]
pub enum BrandFileError {
    Io(std::io::Error),
    NotABrandFile,
    /// Made by a newer version of Lampstand.
    TooNew {
        format: u32,
    },
    Corrupt(String),
    Sync(lampstand_sync::SyncError),
}

impl std::fmt::Display for BrandFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "the file could not be read or written: {e}"),
            Self::NotABrandFile => write!(f, "this is not a Lampstand brand file"),
            Self::TooNew { format } => write!(
                f,
                "this brand file was made by a newer version of Lampstand (file format {format}; this one reads up to {FORMAT_VERSION}). Update Lampstand and try again."
            ),
            Self::Corrupt(why) => write!(f, "the brand file is damaged: {why}"),
            Self::Sync(e) => e.fmt(f),
        }
    }
}

impl From<std::io::Error> for BrandFileError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<lampstand_sync::SyncError> for BrandFileError {
    fn from(e: lampstand_sync::SyncError) -> Self {
        Self::Sync(e)
    }
}
impl From<zip::result::ZipError> for BrandFileError {
    fn from(e: zip::result::ZipError) -> Self {
        match e {
            zip::result::ZipError::Io(e) => Self::Io(e),
            _ => Self::NotABrandFile,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    kind: String,
    format: u32,
    app_version: String,
}

/// What `brand.json` holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrandFile {
    pub brand: Option<Brand>,
    pub idle: Option<IdleScreen>,
    /// The theme made from the brand, one of `themes`.
    pub brand_theme: Option<ThemeId>,
    pub themes: Vec<Theme>,
}

/// What the device keeps about the brand, to put in the file.
#[derive(Debug, Clone, Default)]
pub struct BrandSource {
    pub brand: Option<Brand>,
    pub idle: Option<IdleScreen>,
    pub brand_theme: Option<ThemeId>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ExportReport {
    pub themes: usize,
    /// Pictures and videos bundled, the logo included.
    pub files: usize,
    /// Plain sentences about what was left out.
    pub skipped: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportMode {
    AddAlongside,
    Replace,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportResult {
    /// The brand in the file, its logo now in this library; `None` if the
    /// file has none.
    pub brand: Option<Brand>,
    pub idle: Option<IdleScreen>,
    /// The new ID of the theme made from the brand.
    pub brand_theme: Option<ThemeId>,
    pub themes_added: usize,
    pub themes_replaced: usize,
}

pub fn export(
    library: &Library,
    source: &BrandSource,
    out: impl Write + Seek,
) -> Result<ExportReport, BrandFileError> {
    export_with_limit(library, source, out, MAX_VIDEO_BYTES)
}

fn export_with_limit(
    library: &Library,
    source: &BrandSource,
    out: impl Write + Seek,
    video_limit: u64,
) -> Result<ExportReport, BrandFileError> {
    let mut zip = ZipWriter::new(out);
    let mut report = ExportReport::default();
    let mut bundled: HashSet<BlobHash> = HashSet::new();
    let mut file = BrandFile {
        brand: source.brand.clone(),
        idle: source.idle.clone(),
        brand_theme: source.brand_theme,
        themes: Vec::new(),
    };

    // Puts one file in the zip once; false (with a message) if it can't be.
    let mut bundle = |zip: &mut ZipWriter<_>,
                      report: &mut ExportReport,
                      blob: &BlobHash,
                      what: &str,
                      video: bool|
     -> Result<bool, BrandFileError> {
        if bundled.contains(blob) {
            return Ok(true);
        }
        let Some(path) = library.repo().blobs().path(blob) else {
            report.skipped.push(format!(
                "{what} is not on this computer, so it was left out."
            ));
            return Ok(false);
        };
        let size = std::fs::metadata(&path)?.len();
        if video && size > video_limit {
            report.skipped.push(format!(
                "{what} is {} MB, over the {} MB limit for a brand file, so it was left out.",
                size / (1024 * 1024),
                video_limit / (1024 * 1024)
            ));
            return Ok(false);
        }
        // Pictures and video are compressed already.
        zip.start_file(
            format!("blobs/{blob}"),
            SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
        )?;
        std::io::copy(&mut std::fs::File::open(path)?, zip)?;
        bundled.insert(*blob);
        report.files += 1;
        Ok(true)
    };

    if let Some(logo) = file.brand.as_ref().and_then(|b| b.logo) {
        if !bundle(&mut zip, &mut report, &logo, "The logo", false)? {
            file.brand.as_mut().expect("it has a logo").logo = None;
        }
    }
    match file.idle.clone() {
        Some(IdleScreen::Image { blob, name }) => {
            if !bundle(
                &mut zip,
                &mut report,
                &blob,
                &format!("The idle picture {name}"),
                false,
            )? {
                file.idle = None;
            }
        }
        Some(IdleScreen::Video { blob, name }) => {
            if !bundle(
                &mut zip,
                &mut report,
                &blob,
                &format!("The idle video {name}"),
                true,
            )? {
                file.idle = None;
            }
        }
        Some(IdleScreen::DeckSlide { .. }) => {
            report.skipped.push(
                "The idle screen is a deck slide, which can't travel in a brand file. Choose a picture or video to include it.".into(),
            );
            file.idle = None;
        }
        Some(IdleScreen::Blank | IdleScreen::Default) | None => {}
    }

    let starter = starter_theme();
    for mut theme in library.themes()? {
        if theme == starter {
            continue;
        }
        let name = theme.name.clone();
        let media = match theme.background {
            Background::Image(blob) => Some((blob, "picture", false)),
            Background::Video(blob) => Some((blob, "video", true)),
            _ => None,
        };
        if let Some((blob, kind, video)) = media {
            let what = format!("The background {kind} of “{name}”");
            if !bundle(&mut zip, &mut report, &blob, &what, video)? {
                theme.background = Background::None;
            }
        }
        file.themes.push(theme);
    }
    // The theme made from the brand may be the untouched Starter, or gone.
    if file
        .brand_theme
        .is_some_and(|id| !file.themes.iter().any(|t| t.id == id))
    {
        file.brand_theme = None;
    }
    report.themes = file.themes.len();

    let json = SimpleFileOptions::default();
    zip.start_file("manifest.json", json)?;
    zip.write_all(
        &serde_json::to_vec_pretty(&Manifest {
            kind: "brand".into(),
            format: FORMAT_VERSION,
            app_version: env!("CARGO_PKG_VERSION").into(),
        })
        .map_err(|e| BrandFileError::Corrupt(e.to_string()))?,
    )?;
    zip.start_file("brand.json", json)?;
    zip.write_all(
        &serde_json::to_vec_pretty(&file).map_err(|e| BrandFileError::Corrupt(e.to_string()))?,
    )?;
    zip.finish()?;
    Ok(report)
}

fn read_json<T: serde::de::DeserializeOwned, R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    name: &str,
) -> Result<T, BrandFileError> {
    let mut bytes = Vec::new();
    zip.by_name(name)
        .map_err(|_| BrandFileError::NotABrandFile)?
        .read_to_end(&mut bytes)?;
    serde_json::from_slice(&bytes).map_err(|e| BrandFileError::Corrupt(format!("{name}: {e}")))
}

/// What a file holds, without changing anything.
pub fn inspect(input: impl Read + Seek) -> Result<BrandFile, BrandFileError> {
    let mut zip = ZipArchive::new(input)?;
    let manifest: Manifest = read_json(&mut zip, "manifest.json")?;
    if manifest.kind != "brand" {
        return Err(BrandFileError::NotABrandFile);
    }
    if manifest.format > FORMAT_VERSION {
        return Err(BrandFileError::TooNew {
            format: manifest.format,
        });
    }
    read_json(&mut zip, "brand.json")
}

/// Puts a file's files and themes in the library. The caller applies
/// `brand`, `idle` and `brand_theme` to the device's settings.
pub fn import(
    library: &Library,
    input: impl Read + Seek,
    mode: ImportMode,
) -> Result<ImportResult, BrandFileError> {
    let mut zip = ZipArchive::new(input)?;
    let manifest: Manifest = read_json(&mut zip, "manifest.json")?;
    if manifest.kind != "brand" {
        return Err(BrandFileError::NotABrandFile);
    }
    if manifest.format > FORMAT_VERSION {
        return Err(BrandFileError::TooNew {
            format: manifest.format,
        });
    }
    let file: BrandFile = read_json(&mut zip, "brand.json")?;

    // Every file the brand names, checked against its hash, before any
    // theme is written.
    let mut wanted: Vec<BlobHash> = Vec::new();
    wanted.extend(file.brand.as_ref().and_then(|b| b.logo));
    if let Some(IdleScreen::Image { blob, .. } | IdleScreen::Video { blob, .. }) = &file.idle {
        wanted.push(*blob);
    }
    for theme in &file.themes {
        if let Background::Image(b) | Background::Video(b) = theme.background {
            wanted.push(b);
        }
    }
    let mut seen = HashSet::new();
    for blob in wanted.into_iter().filter(|b| seen.insert(*b)) {
        let mut bytes = Vec::new();
        zip.by_name(&format!("blobs/{blob}"))
            .map_err(|_| BrandFileError::Corrupt(format!("a file is missing ({blob})")))?
            .read_to_end(&mut bytes)?;
        if BlobHash::of(&bytes) != blob {
            return Err(BrandFileError::Corrupt(format!(
                "a file does not match its checksum ({blob})"
            )));
        }
        library.repo().blobs().put(&bytes)?;
    }

    let existing = library.themes()?;
    let starter = crate::starter_theme_id();
    let mut taken: Vec<String> = existing.iter().map(|t| t.name.to_lowercase()).collect();
    let mut result = ImportResult {
        brand: file.brand.clone(),
        idle: file.idle.clone(),
        brand_theme: None,
        themes_added: 0,
        themes_replaced: 0,
    };
    for mut theme in file.themes {
        let old = theme.id;
        let target = (mode == ImportMode::Replace)
            .then(|| {
                existing
                    .iter()
                    .find(|t| t.id != starter && t.name.eq_ignore_ascii_case(&theme.name))
            })
            .flatten();
        match target {
            Some(t) => {
                theme.id = t.id;
                result.themes_replaced += 1;
            }
            None => {
                theme.id = ThemeId::generate();
                if taken.contains(&theme.name.to_lowercase()) {
                    let base = theme.name.clone();
                    let mut n = 2;
                    while taken.contains(&format!("{base} {n}").to_lowercase()) {
                        n += 1;
                    }
                    theme.name = format!("{base} {n}");
                }
                taken.push(theme.name.to_lowercase());
                result.themes_added += 1;
            }
        }
        if file.brand_theme == Some(old) {
            result.brand_theme = Some(theme.id);
        }
        library.put_theme(&theme)?;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lampstand_core::brand::{BugCorner, BugSize, LogoBug};
    use lampstand_core::geom::Color;
    use lampstand_sync::team::DeviceKeypair;
    use std::io::Cursor;

    fn open(dir: &std::path::Path) -> Library {
        Library::open(dir, DeviceKeypair::generate()).unwrap()
    }

    fn brand(logo: Option<BlobHash>) -> Brand {
        Brand {
            name: "Grace Chapel".into(),
            logo,
            primary: Color::rgb(20, 30, 90),
            text: None,
            accent: Color::rgb(240, 190, 60),
            font_family: "Noto Serif".into(),
            bug: LogoBug {
                enabled: true,
                corner: BugCorner::LowerLeft,
                size: BugSize::Large,
                opacity: 55,
            },
        }
    }

    /// A library with a brand theme, a theme with a picture and an idle video.
    fn church(dir: &std::path::Path) -> (Library, BrandSource) {
        let lib = open(dir);
        let logo = lib.repo().blobs().put(b"logo bytes").unwrap();
        let picture = lib.repo().blobs().put(b"picture bytes").unwrap();
        let video = lib.repo().blobs().put(b"video bytes").unwrap();
        let b = brand(Some(logo));
        let made = crate::theme_from_brand(&b, ThemeId::generate());
        lib.put_theme(&made).unwrap();
        let mut photo = starter_theme();
        photo.id = ThemeId::generate();
        photo.name = "Photo".into();
        photo.background = Background::Image(picture);
        lib.put_theme(&photo).unwrap();
        let source = BrandSource {
            brand: Some(b),
            idle: Some(IdleScreen::Video {
                blob: video,
                name: "Loop.mp4".into(),
            }),
            brand_theme: Some(made.id),
        };
        (lib, source)
    }

    fn packed(lib: &Library, source: &BrandSource) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        export(lib, source, &mut out).unwrap();
        out.into_inner()
    }

    /// A theme without its ID, to compare copies.
    fn bare(mut t: Theme) -> Theme {
        t.id = ThemeId::from_uuid(uuid::Uuid::nil());
        t
    }

    fn by_name(lib: &Library) -> Vec<Theme> {
        let mut all: Vec<Theme> = lib.themes().unwrap().into_iter().map(bare).collect();
        all.sort_by(|a, b| a.name.cmp(&b.name));
        all
    }

    #[test]
    fn a_brand_round_trips_into_an_empty_library() {
        let (a, source) = church(tempfile::tempdir().unwrap().keep().as_path());
        let bytes = packed(&a, &source);
        let file = inspect(Cursor::new(&bytes)).unwrap();
        assert_eq!(file.themes.len(), 2, "the untouched Starter is not packed");

        let b = open(tempfile::tempdir().unwrap().keep().as_path());
        let got = import(&b, Cursor::new(&bytes), ImportMode::AddAlongside).unwrap();
        assert_eq!((got.themes_added, got.themes_replaced), (2, 0));
        assert_eq!(got.brand, source.brand);
        assert_eq!(got.idle, source.idle);
        // Equal themes, new IDs, the Starter untouched beside them.
        let mut sent: Vec<Theme> = a
            .themes()
            .unwrap()
            .into_iter()
            .filter(|t| t.id != crate::starter_theme_id())
            .map(bare)
            .collect();
        let mut received: Vec<Theme> = b
            .themes()
            .unwrap()
            .into_iter()
            .filter(|t| t.id != crate::starter_theme_id())
            .map(bare)
            .collect();
        sent.sort_by(|x, y| x.name.cmp(&y.name));
        received.sort_by(|x, y| x.name.cmp(&y.name));
        assert_eq!(sent, received);
        assert!(b
            .themes()
            .unwrap()
            .iter()
            .all(|t| Some(t.id) != source.brand_theme));
        let made = b.theme(got.brand_theme.unwrap()).unwrap().unwrap();
        assert_eq!(made.name, "Grace Chapel");
        // The files arrived with their bytes.
        let read = |lib: &Library, h: &BlobHash| {
            std::fs::read(lib.repo().blobs().path(h).unwrap()).unwrap()
        };
        let got_brand = got.brand.unwrap();
        assert_eq!(
            got_brand.bug,
            LogoBug {
                enabled: true,
                corner: BugCorner::LowerLeft,
                size: BugSize::Large,
                opacity: 55,
            },
            "the logo bug travels with the brand"
        );
        let logo = got_brand.logo.unwrap();
        assert_eq!(read(&b, &logo), b"logo bytes");
        let Some(IdleScreen::Video { blob, .. }) = got.idle else {
            panic!()
        };
        assert_eq!(read(&b, &blob), b"video bytes");
    }

    #[test]
    fn replace_does_not_duplicate_and_add_alongside_renames() {
        let (a, source) = church(tempfile::tempdir().unwrap().keep().as_path());
        let bytes = packed(&a, &source);
        let b = open(tempfile::tempdir().unwrap().keep().as_path());
        import(&b, Cursor::new(&bytes), ImportMode::Replace).unwrap();
        let after_first = by_name(&b);
        assert_eq!(after_first.len(), 3);
        let again = import(&b, Cursor::new(&bytes), ImportMode::Replace).unwrap();
        assert_eq!((again.themes_added, again.themes_replaced), (0, 2));
        assert_eq!(by_name(&b), after_first, "nothing duplicated or changed");

        let ids: Vec<_> = b.themes().unwrap().into_iter().map(|t| t.id).collect();
        let more = import(&b, Cursor::new(&bytes), ImportMode::AddAlongside).unwrap();
        assert_eq!(more.themes_added, 2);
        let names: Vec<String> = b.themes().unwrap().into_iter().map(|t| t.name).collect();
        assert!(
            names.contains(&"Photo 2".to_string()) && names.contains(&"Grace Chapel 2".to_string())
        );
        assert_eq!(b.themes().unwrap().len(), ids.len() + 2);
    }

    #[test]
    fn replace_never_overwrites_the_starter_theme() {
        let (a, source) = church(tempfile::tempdir().unwrap().keep().as_path());
        let mut starterish = starter_theme();
        starterish.id = ThemeId::generate();
        starterish.name = "Starter".into();
        starterish.text.size = 10.0;
        a.put_theme(&starterish).unwrap();
        let bytes = packed(&a, &source);
        let b = open(tempfile::tempdir().unwrap().keep().as_path());
        import(&b, Cursor::new(&bytes), ImportMode::Replace).unwrap();
        let starter = b.theme(crate::starter_theme_id()).unwrap().unwrap();
        assert_eq!(starter, starter_theme());
        assert!(b.themes().unwrap().iter().any(|t| t.name == "Starter 2"));
    }

    #[test]
    fn a_video_over_the_limit_is_left_out_with_a_message() {
        let (a, source) = church(tempfile::tempdir().unwrap().keep().as_path());
        let mut out = Cursor::new(Vec::new());
        let report = export_with_limit(&a, &source, &mut out, 4).unwrap();
        assert_eq!(report.skipped.len(), 1, "{:?}", report.skipped);
        assert!(report.skipped[0].contains("Loop.mp4") && report.skipped[0].contains("limit"));
        let file = inspect(Cursor::new(out.into_inner())).unwrap();
        assert_eq!(file.idle, None);
        assert!(
            file.brand.unwrap().logo.is_some(),
            "pictures are not limited"
        );
    }

    #[test]
    fn a_newer_file_and_a_stranger_are_refused_plainly() {
        let mut out = Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut out);
            zip.start_file("manifest.json", SimpleFileOptions::default())
                .unwrap();
            zip.write_all(br#"{"kind":"brand","format":99,"app_version":"9.0.0"}"#)
                .unwrap();
            zip.finish().unwrap();
        }
        let err = inspect(Cursor::new(out.into_inner())).unwrap_err();
        assert!(matches!(err, BrandFileError::TooNew { format: 99 }));
        assert!(err.to_string().contains("newer version"));
        let err = inspect(Cursor::new(b"not a zip".to_vec())).unwrap_err();
        assert!(matches!(err, BrandFileError::NotABrandFile));
    }

    #[test]
    fn a_file_that_does_not_match_its_checksum_is_refused_before_anything_is_added() {
        let (a, source) = church(tempfile::tempdir().unwrap().keep().as_path());
        let good = packed(&a, &source);
        // Rebuild the zip with the logo's bytes changed.
        let mut zin = ZipArchive::new(Cursor::new(good)).unwrap();
        let mut out = Cursor::new(Vec::new());
        {
            let mut zout = ZipWriter::new(&mut out);
            for i in 0..zin.len() {
                let mut f = zin.by_index(i).unwrap();
                let name = f.name().unwrap().to_string();
                let mut bytes = Vec::new();
                f.read_to_end(&mut bytes).unwrap();
                if bytes == b"logo bytes" {
                    bytes = b"tampered!!".to_vec();
                }
                zout.start_file(name, SimpleFileOptions::default()).unwrap();
                zout.write_all(&bytes).unwrap();
            }
            zout.finish().unwrap();
        }
        let b = open(tempfile::tempdir().unwrap().keep().as_path());
        let err = import(&b, Cursor::new(out.into_inner()), ImportMode::AddAlongside).unwrap_err();
        assert!(matches!(err, BrandFileError::Corrupt(_)));
        assert_eq!(b.themes().unwrap().len(), 1, "only the Starter");
    }
}
