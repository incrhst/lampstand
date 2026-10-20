//! Repo maintenance tasks, run as `cargo xtask <task>`.
//!
//! - `bibles`: downloads the bundled public-domain Bibles (KJV, WEB) from
//!   eBible.org, imports them with the USFM importer and writes the
//!   postcard + zstd blobs the app installs on first run to
//!   `assets/bibles/`, with a README recording sources and licences.

use std::fmt::Write as _;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use lampstand_core::scripture::BibleText;
use lampstand_import::{import_bible, BibleFormat};
use lampstand_library::encode_bible_text;

/// One Bible to bundle. `file` must match `lampstand_library::bundled`.
struct Source {
    /// eBible.org translation ID.
    ebible_id: &'static str,
    file: &'static str,
    name: &'static str,
    licence: &'static str,
}

const SOURCES: [Source; 2] = [
    Source {
        ebible_id: "eng-kjv",
        file: "kjv.postcard.zst",
        name: "King James Version (1769 standard text)",
        licence: "Public domain. eBible.org: \"Letters patent issued by King James with no \
                  expiration date means that to print this translation in the United Kingdom \
                  or import printed copies into the UK, you need permission. [...] This royal \
                  decree has no effect outside of the UK, where this work is firmly in the \
                  Public Domain.\" Text courtesy of the CrossWire Bible Society and \
                  eBible.org. The source also carries the Apocrypha; only the 66 books of \
                  the Protestant canon are kept.",
    },
    Source {
        ebible_id: "engwebp",
        file: "web.postcard.zst",
        name: "World English Bible (66-book protocanon)",
        licence: "Public domain. \"World English Bible\" is a trademark of eBible.org: the \
                  text may be used freely, but a changed text must not be called the World \
                  English Bible. Lampstand stores the text as published, minus footnotes \
                  and cross references (which are not verse text).",
    },
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("bibles") => bibles(),
        _ => {
            eprintln!("usage: cargo xtask bibles");
            return ExitCode::FAILURE;
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask: {e}");
            ExitCode::FAILURE
        }
    }
}

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask is one level below the repo root")
        .to_path_buf()
}

fn bibles() -> Result<()> {
    let out_dir = repo_root().join("assets/bibles");
    std::fs::create_dir_all(&out_dir)?;
    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!("lampstand-xtask/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let mut readme = String::from(README_HEAD);

    for source in &SOURCES {
        let url = format!(
            "https://ebible.org/Scriptures/{}_usfm.zip",
            source.ebible_id
        );
        eprintln!("downloading {url}");
        let response = client.get(&url).send()?.error_for_status()?;
        let last_modified = response
            .headers()
            .get(reqwest::header::LAST_MODIFIED)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("unknown")
            .to_string();
        let zip_bytes = response.bytes()?;

        let dir = tempfile::tempdir()?;
        zip::ZipArchive::new(Cursor::new(&zip_bytes[..]))?.extract(dir.path())?;
        let import = import_bible(BibleFormat::Usfm, dir.path())
            .map_err(|e| format!("{}: import failed: {e:?}", source.ebible_id))?;
        for warning in &import.warnings {
            eprintln!("  {}: {warning}", source.ebible_id);
        }
        let mut text = import.text;
        clean(&mut text);
        let (books, verses) = count(&text);
        if books != 66 {
            return Err(format!("{}: expected 66 books, got {books}", source.ebible_id).into());
        }

        let blob = encode_bible_text(&text)?;
        std::fs::write(out_dir.join(source.file), &blob)?;
        eprintln!(
            "  wrote {} ({} bytes, {verses} verses)",
            source.file,
            blob.len()
        );

        writeln!(readme, "## `{}`\n", source.file)?;
        writeln!(readme, "- Text: {}", source.name)?;
        writeln!(
            readme,
            "- Source: <{url}> (eBible.org ID `{}`)",
            source.ebible_id
        )?;
        writeln!(readme, "- Source `Last-Modified`: {last_modified}")?;
        writeln!(
            readme,
            "- Source zip BLAKE3: `{}`",
            blake3::hash(&zip_bytes).to_hex()
        )?;
        writeln!(readme, "- {books} books, {verses} verses")?;
        writeln!(readme, "- Licence: {}\n", source.licence)?;
    }

    std::fs::write(out_dir.join("README.md"), readme)?;
    Ok(())
}

const README_HEAD: &str = "\
# Bundled Bibles

Public-domain translations every fresh install has, so scripture works
before anyone imports a Bible (PRD BI-2). The app embeds these files and
installs them on first run (`lampstand_library::bundled`).

Each file is a `BibleText` (`books[b][c][v]`) as postcard, then zstd: the
same bytes as an imported translation's blob
(`lampstand_library::encode_bible_text`).

Generated by `cargo xtask bibles` (see `xtask/src/main.rs`), which
downloads eBible.org's USFM, imports it with the USFM importer and drops
the KJV's paragraph marks (¶). Do not edit by hand; rerun the task.

";

/// Tidies imported verse text for the screen. The KJV marks paragraph
/// starts with a pilcrow, which is typography rather than text.
fn clean(text: &mut BibleText) {
    for verse in text.books.iter_mut().flatten().flatten() {
        if verse.contains('¶') {
            *verse = verse
                .replace('¶', "")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
        }
    }
}

/// Books present and non-empty verses.
fn count(text: &BibleText) -> (usize, usize) {
    let books = text.books.iter().filter(|b| !b.is_empty()).count();
    let verses = text
        .books
        .iter()
        .flatten()
        .flatten()
        .filter(|v| !v.is_empty())
        .count();
    (books, verses)
}
