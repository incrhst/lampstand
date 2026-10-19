//! `LibraryRequest::ImportFiles`: works out what each dropped or picked
//! file is, by extension and then by its first bytes, and hands it to the
//! matching `lampstand-import` importer. Best effort, like the importers:
//! a file we cannot read is reported, never a failed import.

use std::io::Read;
use std::path::Path;

use lampstand_api::Event;
use lampstand_core::scripture::TranslationId;
use lampstand_core::song::{lyrics, Song, SongId};
use lampstand_import::{import_bible, import_songs, BibleFormat, ImportError, SongFormat};
use lampstand_library::{encode_bible_text, Library};

/// What a file holds, as far as we can tell without parsing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detected {
    Songs(SongFormat),
    Bible(BibleFormat),
    /// Decks, images and video: imported by later tickets (T29+, T27).
    NotYet(&'static str),
    Unknown,
}

/// Detects by extension first, then by sniffing the start of the file.
pub fn detect(path: &Path) -> Detected {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match ext.as_str() {
        "usr" => return Detected::Songs(SongFormat::SongSelect),
        "show" => return Detected::Songs(SongFormat::FreeShow),
        "pro" | "pro6" => return Detected::Songs(SongFormat::ProPresenter),
        "usfm" | "sfm" => return Detected::Bible(BibleFormat::Usfm),
        "pptx" | "ppt" | "key" | "odp" => return Detected::NotYet("presentations"),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" => return Detected::NotYet("images"),
        "mp4" | "mov" | "m4v" | "webm" | "mkv" => return Detected::NotYet("video"),
        _ => {}
    }
    if path.is_dir() {
        return detect_folder(path);
    }
    let head = head(path);
    let lower = head.to_ascii_lowercase();
    if ext == "osb" || head.starts_with("PK\u{3}\u{4}") {
        // An OpenSongApp backup (a zip of OpenSong songs), as published by
        // worshipleaderapp.com. Other zips (PPTX) were caught by extension.
        return Detected::Songs(SongFormat::OpenSong);
    }
    if head.starts_with("SQLite format 3") {
        // OpenLP Bibles have a `verse` table; songs.sqlite is OpenLP's
        // song database (T06, not built) and EasyWorship uses SQLite too.
        return if lower.contains("create table verse") {
            Detected::Bible(BibleFormat::OpenLp)
        } else if lower.contains("create table song_words") || lower.contains("create table word") {
            Detected::Songs(SongFormat::EasyWorship)
        } else {
            Detected::Unknown
        };
    }
    if lower.contains("openlyrics.info/namespace") {
        Detected::Songs(SongFormat::OpenLyrics)
    } else if lower.contains("<osis") {
        Detected::Bible(BibleFormat::Osis)
    } else if lower.contains("<xmlbible") {
        Detected::Bible(BibleFormat::Zefania)
    } else if lower.contains("<bible") {
        Detected::Bible(BibleFormat::OpenSong)
    } else if lower.contains("<song") && lower.contains("<lyrics") {
        Detected::Songs(SongFormat::OpenSong)
    } else if lower.contains("ccli song #") || lower.contains("ccli song number") {
        Detected::Songs(SongFormat::SongSelect)
    } else {
        Detected::Unknown
    }
}

/// A folder: USFM books, or an OpenSong library (its data folder, its
/// `Songs` folder or any folder of song files), told apart by sampling the
/// files in it.
fn detect_folder(path: &Path) -> Detected {
    if songs_folder(path).is_some() {
        return Detected::Songs(SongFormat::OpenSong);
    }
    let entries: Vec<std::path::PathBuf> = std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .collect();
    let usfm = entries.iter().any(|p| {
        matches!(
            p.extension()
                .and_then(|x| x.to_str())
                .map(str::to_ascii_lowercase)
                .as_deref(),
            Some("usfm" | "sfm")
        )
    });
    if usfm {
        return Detected::Bible(BibleFormat::Usfm);
    }
    // OpenSong songs are XML files with no extension; a sample is enough.
    // Song folders may be nested by category.
    let opensong = Detected::Songs(SongFormat::OpenSong);
    let has_songs = entries
        .iter()
        .filter(|p| p.is_file())
        .take(20)
        .any(|p| detect(p) == opensong)
        || entries
            .iter()
            .filter(|p| p.is_dir())
            .any(|p| detect_folder(p) == opensong);
    if has_songs {
        opensong
    } else {
        Detected::Unknown
    }
}

/// OpenSong's data folder holds `Songs` beside `Sets` and `Backgrounds`;
/// only `Songs` is imported, so sets and images are not reported as bad
/// songs.
fn songs_folder(path: &Path) -> Option<std::path::PathBuf> {
    let songs = path.join("Songs");
    (songs.is_dir() && path.join("Sets").is_dir()).then_some(songs)
}

/// The first 8 KiB as text, lossily (enough for an XML root element or an
/// SQLite schema page).
fn head(path: &Path) -> String {
    let mut buf = vec![0; 8192];
    let n = std::fs::File::open(path)
        .and_then(|mut f| f.read(&mut buf))
        .unwrap_or(0);
    buf.truncate(n);
    String::from_utf8_lossy(&buf).into_owned()
}

/// Imported songs get an ID derived from their title and lyrics, so
/// importing the same file or database again (a second sync, the same
/// OpenSong folder twice) finds the songs already there instead of adding
/// copies. Songs already in the library are left as they are, edits and
/// all.
pub fn content_id(song: &Song) -> SongId {
    const NAMESPACE: uuid::Uuid = uuid::Uuid::from_u128(0x6c61_6d70_7374_5000_8000_736f_6e67_7331);
    let normal = |text: &str| {
        text.split_whitespace()
            .map(str::to_lowercase)
            .collect::<Vec<_>>()
            .join(" ")
    };
    let mut key = normal(&song.title);
    for section in &song.sections {
        for line in &section.lines {
            key.push('\n');
            key.push_str(&normal(&lyrics(line)));
        }
    }
    SongId::from_uuid(uuid::Uuid::new_v5(&NAMESPACE, key.as_bytes()))
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Summary {
    pub songs: usize,
    /// Songs that were in the library already.
    pub existing: usize,
    pub bibles: Vec<String>,
    /// "file: reason" for everything that was not imported.
    pub skipped: Vec<String>,
}

impl Summary {
    /// One line for the operator.
    pub fn message(&self) -> String {
        let mut parts = vec![format!(
            "Imported {} song{}",
            self.songs,
            if self.songs == 1 { "" } else { "s" }
        )];
        if self.existing > 0 {
            parts.push(format!("{} already in the library", self.existing));
        }
        if !self.bibles.is_empty() {
            parts.push(format!("Bibles: {}", self.bibles.join(", ")));
        }
        if !self.skipped.is_empty() {
            const SHOWN: usize = 5;
            let mut reasons = self.skipped[..self.skipped.len().min(SHOWN)].join("; ");
            if self.skipped.len() > SHOWN {
                reasons.push_str(&format!("; and {} more", self.skipped.len() - SHOWN));
            }
            parts.push(format!("skipped {}: {reasons}", self.skipped.len()));
        }
        parts.join(". ")
    }
}

/// `LibraryRequest::ImportFiles`: imports each path and returns the
/// summary line. Reports progress through `emit` at least every
/// [`PROGRESS_EVERY`] songs, since a song database takes minutes.
///
/// Takes the library, not the app: the shell runs this off the `App` lock
/// so live control keeps working during a long import.
pub fn import_files(library: &Library, paths: &[String], emit: &dyn Fn(Event)) -> String {
    let mut summary = Summary::default();
    let total = paths.len();
    for (done, path) in paths.iter().enumerate() {
        emit(Event::ImportProgress {
            done,
            total,
            message: format!("Reading {path}"),
        });
        import_path(library, Path::new(path), &mut summary, &|songs, of| {
            emit(Event::ImportProgress {
                done,
                total,
                message: format!("Adding songs: {songs} of {of}"),
            })
        });
    }
    let message = summary.message();
    emit(Event::ImportProgress {
        done: total,
        total,
        message: message.clone(),
    });
    message
}

pub const PROGRESS_EVERY: usize = 100;

/// Imports one file or folder into the library. `progress` hears (songs
/// stored so far, songs in this file) every [`PROGRESS_EVERY`] songs.
pub fn import_path(
    library: &Library,
    path: &Path,
    summary: &mut Summary,
    progress: &dyn Fn(usize, usize),
) {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let failed = |e: ImportError| match e {
        ImportError::Io(e) => e.to_string(),
        ImportError::WrongFormat => "not the format it looked like".into(),
        ImportError::Unreadable(why) => why,
    };
    match detect(path) {
        Detected::Songs(format) => match import_songs(
            format,
            &songs_folder(path).unwrap_or_else(|| path.to_path_buf()),
        ) {
            Ok(import) => {
                let of = import.songs.len();
                for (i, mut song) in import.songs.into_iter().enumerate() {
                    if i % PROGRESS_EVERY == 0 && i > 0 {
                        progress(i, of);
                    }
                    song.id = content_id(&song);
                    let stored = library.song(song.id).and_then(|existing| match existing {
                        Some(_) => Ok(false),
                        None => library.put_song(&song).map(|()| true),
                    });
                    match stored {
                        Ok(true) => summary.songs += 1,
                        Ok(false) => summary.existing += 1,
                        Err(e) => summary.skipped.push(format!("{}: {e}", song.title)),
                    }
                }
                summary.skipped.extend(
                    import
                        .skipped
                        .into_iter()
                        .map(|s| format!("{}: {}", s.source, s.reason)),
                );
            }
            Err(e) => summary.skipped.push(format!("{name}: {}", failed(e))),
        },
        Detected::Bible(format) => match import_bible(format, path) {
            Ok(import) => {
                let stored = encode_bible_text(&import.text)
                    .and_then(|bytes| library.repo().blobs().put(&bytes))
                    .and_then(|hash| {
                        let mut translation = import.translation;
                        translation.id = TranslationId::generate();
                        translation.text = hash;
                        library.put_translation(&translation)?;
                        Ok(translation.abbreviation)
                    });
                match stored {
                    Ok(abbreviation) => summary.bibles.push(abbreviation),
                    Err(e) => summary.skipped.push(format!("{name}: {e}")),
                }
            }
            Err(e) => summary.skipped.push(format!("{name}: {}", failed(e))),
        },
        Detected::NotYet(what) => summary
            .skipped
            .push(format!("{name}: importing {what} is not built yet")),
        Detected::Unknown => summary.skipped.push(format!(
            "{name}: not a song or Bible format Lampstand knows"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(rel: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/lampstand-import/tests/fixtures")
            .join(rel)
    }

    #[test]
    fn detects_the_fixture_formats() {
        let cases = [
            (
                "openlyrics/amazing-grace.xml",
                Detected::Songs(SongFormat::OpenLyrics),
            ),
            (
                "opensong/Amazing Grace",
                Detected::Songs(SongFormat::OpenSong),
            ),
            (
                "songselect/lamp-for-the-path.usr",
                Detected::Songs(SongFormat::SongSelect),
            ),
            (
                "songselect/river-of-peace.txt",
                Detected::Songs(SongFormat::SongSelect),
            ),
            (
                "freeshow/amazing-grace.show",
                Detected::Songs(SongFormat::FreeShow),
            ),
            (
                "propresenter/Amazing Grace.pro6",
                Detected::Songs(SongFormat::ProPresenter),
            ),
            (
                "osis/kjv-container.osis.xml",
                Detected::Bible(BibleFormat::Osis),
            ),
            (
                "zefania/kjv.zefania.xml",
                Detected::Bible(BibleFormat::Zefania),
            ),
            (
                "opensong_bible/kjv_sample.xml",
                Detected::Bible(BibleFormat::OpenSong),
            ),
            ("usfm", Detected::Bible(BibleFormat::Usfm)),
            ("openlyrics/readme.txt", Detected::Unknown),
        ];
        for (rel, want) in cases {
            assert_eq!(detect(&fixture(rel)), want, "{rel}");
        }
        assert_eq!(
            detect(Path::new("talk.pptx")),
            Detected::NotYet("presentations")
        );
    }
}
