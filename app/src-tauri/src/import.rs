//! `LibraryRequest::ImportFiles`: works out what each dropped or picked
//! file is, by extension and then by its first bytes, and hands it to the
//! matching `lampstand-import` importer. Best effort, like the importers:
//! a file we cannot read is reported, never a failed import.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use lampstand_api::Event;
use lampstand_core::id::Timestamp;
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

/// `LibraryRequest::AddSongFromLink`, once the shell has the page: reads the
/// song on it and adds it to the library. A song with the same words, or
/// the same CCLI number, is already there, and that song is the answer.
pub fn add_song_page(library: &Library, html: &str) -> Result<Song, String> {
    let mut song = lampstand_import::songs::web::parse_page(html)?;
    song.id = content_id(&song);
    if let Some(existing) = library.song(song.id).map_err(|e| e.to_string())? {
        return Ok(existing);
    }
    if let Some(n) = song.ccli_number {
        let all = library.songs().map_err(|e| e.to_string())?;
        if let Some(existing) = all.into_iter().find(|s| s.ccli_number == Some(n)) {
            return Ok(existing);
        }
    }
    library.put_song(&song).map_err(|e| e.to_string())?;
    Ok(song)
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Summary {
    pub songs: usize,
    /// Songs that were in the library already.
    pub existing: usize,
    /// Songs whose CCLI number was in the library and whose words replaced
    /// that song's (SongSelect downloads only, [`OnKnownCcli::Update`]).
    pub updated: usize,
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
        if self.updated > 0 {
            parts.push(format!("{} updated", self.updated));
        }
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

/// What became of one song from a file.
enum Stored {
    New,
    Updated,
    Existing,
}

/// `LibraryRequest::ImportSongSelectDownloads`: imports the SongSelect
/// files in `dir` (not its subfolders) modified at or after `since`. `None`
/// when there is nothing new, so a watcher stays quiet.
///
/// `handled` holds the files a previous look imported, with the modified
/// time they had then: the watcher's windows overlap, and a file it already
/// took is not reported again as "already in the library". A file saved
/// over with a new modified time counts as new.
///
/// A `.txt` is only taken if it carries SongSelect's `CCLI Song #` footer:
/// Downloads is full of other text files.
pub fn import_song_select_downloads(
    library: &Library,
    dir: &Path,
    since: Timestamp,
    handled: &mut HashMap<PathBuf, SystemTime>,
) -> Option<String> {
    let since = UNIX_EPOCH.checked_add(Duration::from_millis(since.0.max(0) as u64))?;
    // Older than the window: never looked at again, so no longer needed.
    handled.retain(|_, modified| *modified >= since);
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter_map(|p| {
            let modified = p.metadata().and_then(|m| m.modified()).ok()?;
            (modified >= since && handled.get(&p) != Some(&modified)).then_some((p, modified))
        })
        .filter(|(p, _)| match p.extension().and_then(|e| e.to_str()) {
            Some(e) if e.eq_ignore_ascii_case("usr") => true,
            Some(e) if e.eq_ignore_ascii_case("txt") => {
                p.metadata().is_ok_and(|m| m.len() < 1 << 20)
                    && std::fs::read(p).is_ok_and(|b| {
                        String::from_utf8_lossy(&b)
                            .to_ascii_lowercase()
                            .contains("ccli song #")
                    })
            }
            _ => false,
        })
        .collect();
    if files.is_empty() {
        return None;
    }
    files.sort();
    let mut summary = Summary::default();
    for (file, modified) in files {
        // SongSelect is where these songs come from, so a download of one
        // already here is the newer words for it.
        import_songs_from(
            library,
            &file,
            &mut summary,
            &|_, _| {},
            OnKnownCcli::Update,
        );
        handled.insert(file, modified);
    }
    Some(summary.message())
}

/// What an import does with a song whose CCLI number is already in the
/// library under different words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnKnownCcli {
    /// Leave the library's song as it is: the file is counted as already in
    /// the library. Every import but SongSelect's.
    Skip,
    /// Put the file's words into that song, keeping its ID, so services
    /// that use it get them. For SongSelect downloads, whose words are the
    /// licensed source; edits made in Lampstand to that song are replaced.
    Update,
}

/// Imports one file or folder into the library. `progress` hears (songs
/// stored so far, songs in this file) every [`PROGRESS_EVERY`] songs. A song
/// whose CCLI number is already in the library is skipped.
pub fn import_path(
    library: &Library,
    path: &Path,
    summary: &mut Summary,
    progress: &dyn Fn(usize, usize),
) {
    import_songs_from(library, path, summary, progress, OnKnownCcli::Skip)
}

/// [`import_path`], choosing what a known CCLI number does.
pub fn import_songs_from(
    library: &Library,
    path: &Path,
    summary: &mut Summary,
    progress: &dyn Fn(usize, usize),
    on_known: OnKnownCcli,
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
                // CCLI numbers already in the library, read once and only if
                // this file has any: a song downloaded again from SongSelect
                // with a corrected line is the same song, not a new one.
                let mut known_ccli: HashMap<u32, SongId> =
                    if import.songs.iter().any(|s| s.ccli_number.is_some()) {
                        library
                            .songs()
                            .map(|all| {
                                all.iter()
                                    .filter_map(|s| s.ccli_number.map(|n| (n, s.id)))
                                    .collect()
                            })
                            .unwrap_or_default()
                    } else {
                        HashMap::new()
                    };
                for (i, mut song) in import.songs.into_iter().enumerate() {
                    if i % PROGRESS_EVERY == 0 && i > 0 {
                        progress(i, of);
                    }
                    song.id = content_id(&song);
                    let known = song.ccli_number.and_then(|n| known_ccli.get(&n).copied());
                    let stored =
                        library
                            .song(song.id)
                            .and_then(|existing| match (existing, known) {
                                (Some(_), _) => Ok(Stored::Existing),
                                (None, Some(_)) if on_known == OnKnownCcli::Skip => {
                                    Ok(Stored::Existing)
                                }
                                (None, Some(id)) => {
                                    song.id = id;
                                    library.put_song(&song).map(|()| Stored::Updated)
                                }
                                (None, None) => library.put_song(&song).map(|()| {
                                    if let Some(n) = song.ccli_number {
                                        known_ccli.insert(n, song.id);
                                    }
                                    Stored::New
                                }),
                            });
                    match stored {
                        Ok(Stored::New) => summary.songs += 1,
                        Ok(Stored::Updated) => summary.updated += 1,
                        Ok(Stored::Existing) => summary.existing += 1,
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

    #[test]
    fn imports_only_new_songselect_files_from_downloads() {
        let dir = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let key = lampstand_sync::team::DeviceKeypair::generate();
        let library = Library::open(data.path(), key).unwrap();
        std::fs::copy(
            fixture("songselect/lamp-for-the-path.usr"),
            dir.path().join("a.usr"),
        )
        .unwrap();
        std::fs::copy(
            fixture("songselect/river-of-peace.txt"),
            dir.path().join("river.txt"),
        )
        .unwrap();
        std::fs::write(dir.path().join("notes.txt"), "shopping list").unwrap();
        std::fs::write(dir.path().join("pic.png"), "x").unwrap();
        let mut handled = HashMap::new();
        let look = |handled: &mut HashMap<_, _>, since| {
            import_song_select_downloads(&library, dir.path(), Timestamp(since), handled)
        };
        let message = look(&mut handled, 0).unwrap();
        assert!(message.starts_with("Imported 2 songs"), "{message}");
        // The next look overlaps this one: what it took stays quiet.
        assert_eq!(look(&mut handled, 0), None);
        // Another copy of a song is still worth a word.
        std::fs::copy(
            fixture("songselect/lamp-for-the-path.usr"),
            dir.path().join("a (1).usr"),
        )
        .unwrap();
        let again = look(&mut handled, 0).unwrap();
        assert!(again.contains("1 already in the library"), "{again}");
        assert_eq!(look(&mut HashMap::new(), i64::MAX), None);
    }

    #[test]
    fn a_song_with_a_known_ccli_number_is_not_added_twice() {
        let dir = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let key = lampstand_sync::team::DeviceKeypair::generate();
        let library = Library::open(data.path(), key).unwrap();
        let original =
            std::fs::read_to_string(fixture("songselect/lamp-for-the-path.usr")).unwrap();
        let first = dir.path().join("first.usr");
        std::fs::write(&first, &original).unwrap();
        // The same song with a lyric corrected: a different content ID.
        let second = dir.path().join("second.usr");
        std::fs::write(
            &second,
            original.replacen("warm and small", "warm and bright", 1),
        )
        .unwrap();
        assert_ne!(original, std::fs::read_to_string(&second).unwrap());
        let mut summary = Summary::default();
        import_path(&library, &first, &mut summary, &|_, _| {});
        import_path(&library, &second, &mut summary, &|_, _| {});
        assert_eq!((summary.songs, summary.existing), (1, 1));
    }

    #[test]
    fn a_song_page_added_twice_is_one_song() {
        let data = tempfile::tempdir().unwrap();
        let key = lampstand_sync::team::DeviceKeypair::generate();
        let library = Library::open(data.path(), key).unwrap();
        let page = |line: &str| {
            format!(
                "<html><head><title>Page Song | Site</title></head><body>
                 <h3>Lyrics</h3><p>VERSE 1<br>{line}<br>Second line</p>
                 <p>CCLI Song # 1234567</p></body></html>"
            )
        };
        let first = add_song_page(&library, &page("First line")).unwrap();
        assert_eq!(first.title, "Page Song");
        assert_eq!(
            add_song_page(&library, &page("First line")).unwrap().id,
            first.id
        );
        // A corrected line is still the song with that CCLI number.
        assert_eq!(
            add_song_page(&library, &page("First line, fixed"))
                .unwrap()
                .id,
            first.id
        );
        assert_eq!(library.songs().unwrap().len(), 1);
        assert!(add_song_page(&library, "<html><body>No song</body></html>").is_err());
    }

    /// A song downloaded again from SongSelect with a line corrected puts the
    /// new words into the library's song, under the same ID, so a service
    /// that uses it gets them. Any other import of that file still skips it.
    #[test]
    fn a_songselect_download_of_a_known_song_updates_it_in_place() {
        let downloads = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let key = lampstand_sync::team::DeviceKeypair::generate();
        let library = Library::open(data.path(), key).unwrap();
        let original =
            std::fs::read_to_string(fixture("songselect/lamp-for-the-path.usr")).unwrap();
        let first = downloads.path().join("first.usr");
        std::fs::write(&first, &original).unwrap();
        let mut summary = Summary::default();
        import_path(&library, &first, &mut summary, &|_, _| {});
        let [song] = library.songs().unwrap().try_into().unwrap();
        assert!(song.ccli_number.is_some(), "the fixture has a CCLI number");
        let words = |s: &Song| format!("{:?}", s.sections);
        assert!(words(&song).contains("warm and small"));

        // Dropped on the library, the corrected file is skipped as before.
        let corrected = downloads.path().join("corrected.usr");
        std::fs::write(
            &corrected,
            original.replacen("warm and small", "warm and bright", 1),
        )
        .unwrap();
        let mut summary = Summary::default();
        import_path(&library, &corrected, &mut summary, &|_, _| {});
        assert_eq!(
            (summary.songs, summary.updated, summary.existing),
            (0, 0, 1)
        );

        // From the SongSelect watcher it updates the song.
        let mut handled = HashMap::new();
        let message =
            import_song_select_downloads(&library, downloads.path(), Timestamp(0), &mut handled)
                .unwrap();
        assert!(message.contains("1 updated"), "{message}");
        let songs = library.songs().unwrap();
        assert_eq!(songs.len(), 1, "still one song, not a copy");
        assert_eq!(songs[0].id, song.id, "the same song");
        assert!(words(&songs[0]).contains("warm and bright"));
    }
}
