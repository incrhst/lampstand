//! FreeShow `.show` JSON files (and the projects folder they live in).
//!
//! A `.show` file is a JSON array `[id, show]`. Schema (ChurchApps/FreeShow,
//! `src/types/Show.ts`, `Show`, `Slide`, `Layout`, `Line`, `Chords`):
//!
//! - `name` is the title; `meta` has `CCLI`, `author` (or `composer`,
//!   `artist`), `copyright` and `key`.
//! - `slides` maps ids to slides with `group`, `globalGroup`, `children`
//!   (ids of continuation slides) and `items[].lines[].text[].value`; a line
//!   may carry `chords: [{pos, key}]`, `pos` being a character offset.
//! - `layouts` maps ids to `{name, slides: [{id, disabled?}]}`; the one named
//!   by `settings.activeLayout` is the arrangement.
//!
//! Global group ids (`verse`, `pre_chorus`, `chorus`, `bridge`, `intro`,
//! `outro`, `tag`, `break`) are the defaults in FreeShow's `lang/en.json`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use lampstand_core::song::{Section, Song};
use serde::Deserialize;

use crate::{ImportError, Skipped, SongImport};

#[derive(Deserialize, Default)]
#[serde(default)]
struct Show {
    name: String,
    settings: Settings,
    meta: Meta,
    slides: BTreeMap<String, Slide>,
    layouts: BTreeMap<String, Layout>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Settings {
    active_layout: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Meta {
    #[serde(rename = "CCLI")]
    ccli: Option<String>,
    author: Option<String>,
    composer: Option<String>,
    artist: Option<String>,
    copyright: Option<String>,
    key: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Slide {
    group: Option<String>,
    global_group: Option<String>,
    children: Vec<String>,
    items: Vec<Item>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Item {
    #[serde(rename = "type")]
    kind: Option<String>,
    lines: Vec<Line>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Line {
    text: Vec<Text>,
    chords: Vec<Chord>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Text {
    value: String,
}

#[derive(Deserialize)]
struct Chord {
    pos: usize,
    key: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Layout {
    slides: Vec<LayoutSlide>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct LayoutSlide {
    id: String,
    disabled: bool,
}

pub fn import(path: &Path) -> Result<SongImport, ImportError> {
    let mut out = SongImport::default();
    if path.is_dir() {
        let mut files = Vec::new();
        collect(path, &mut files)?;
        if files.is_empty() {
            return Err(ImportError::WrongFormat);
        }
        files.sort();
        for file in files {
            import_file(&file, &mut out)?;
        }
    } else {
        import_file(path, &mut out)?;
    }
    Ok(out)
}

fn collect(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), ImportError> {
    for entry in std::fs::read_dir(dir)? {
        let p = entry?.path();
        if p.is_dir() {
            collect(&p, files)?;
        } else if p
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("show"))
        {
            files.push(p);
        }
    }
    Ok(())
}

fn import_file(path: &Path, out: &mut SongImport) -> Result<(), ImportError> {
    let source = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let bytes = std::fs::read(path)?;
    match parse(&bytes) {
        Ok(song) => out.songs.push(song),
        Err(reason) => out.skipped.push(Skipped { source, reason }),
    }
    Ok(())
}

fn parse(bytes: &[u8]) -> Result<Song, String> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|e| format!("not JSON: {e}"))?;
    // `[id, show]`; a bare show object is accepted too.
    let show_value = match value {
        serde_json::Value::Array(mut a) if a.len() >= 2 => a.swap_remove(1),
        v @ serde_json::Value::Object(_) => v,
        _ => return Err("not a FreeShow [id, show] array".into()),
    };
    let show: Show = serde_json::from_value(show_value).map_err(|e| format!("bad show: {e}"))?;
    let title = show.name.trim();
    if title.is_empty() {
        return Err("show has no name".into());
    }
    build(title, &show)
}

fn build(title: &str, show: &Show) -> Result<Song, String> {
    let layout = show
        .settings
        .active_layout
        .as_ref()
        .and_then(|id| show.layouts.get(id))
        .or_else(|| show.layouts.values().next());

    // Slides that continue another slide are folded into it.
    let children: HashSet<&str> = show
        .slides
        .values()
        .flat_map(|s| s.children.iter().map(String::as_str))
        .collect();

    // Order: layout order first, then slides the layout does not use.
    let mut order: Vec<&str> = Vec::new();
    if let Some(l) = layout {
        for ls in &l.slides {
            if show.slides.contains_key(&ls.id) && !order.contains(&ls.id.as_str()) {
                order.push(&ls.id);
            }
        }
    }
    for id in show.slides.keys() {
        if !order.contains(&id.as_str()) {
            order.push(id);
        }
    }

    // (slide id, kind key, label hint, lines) for slides with text.
    let mut sections: Vec<(&str, String, Vec<String>)> = Vec::new();
    for id in order {
        if children.contains(id) {
            continue;
        }
        let slide = &show.slides[id];
        let mut lines = slide_lines(slide);
        for child in &slide.children {
            if let Some(c) = show.slides.get(child) {
                lines.extend(slide_lines(c));
            }
        }
        if lines.is_empty() {
            continue;
        }
        sections.push((id, group_key(slide), lines));
    }
    if sections.is_empty() {
        return Err("no lyrics".into());
    }

    // Label: verses are always numbered; other kinds only when repeated.
    let mut totals: HashMap<&str, usize> = HashMap::new();
    for (_, key, _) in &sections {
        *totals.entry(key).or_default() += 1;
    }
    let mut seen: HashMap<&str, usize> = HashMap::new();
    let mut labels: HashMap<&str, String> = HashMap::new();
    let mut song = Song::new(title);
    for (id, key, lines) in &sections {
        let n = seen.entry(key).or_default();
        *n += 1;
        let base = label_base(key);
        let label = if key == "verse" || totals[key.as_str()] > 1 {
            format!("{base}{n}")
        } else {
            base
        };
        labels.insert(id, label.clone());
        song.sections.push(Section {
            label,
            lines: lines.clone(),
        });
    }

    if let Some(l) = layout {
        song.arrangement = l
            .slides
            .iter()
            .filter(|ls| !ls.disabled)
            .filter_map(|ls| labels.get(ls.id.as_str()).cloned())
            .collect();
    }

    let m = &show.meta;
    song.author = [&m.author, &m.composer, &m.artist]
        .into_iter()
        .find_map(non_empty);
    song.copyright = non_empty(&m.copyright);
    song.key = non_empty(&m.key);
    song.ccli_number = m.ccli.as_deref().and_then(|c| {
        let digits: String = c.chars().filter(char::is_ascii_digit).collect();
        digits.parse().ok()
    });
    Ok(song)
}

fn non_empty(v: &Option<String>) -> Option<String> {
    v.as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

/// Lines of text items, chords inlined as ChordPro.
fn slide_lines(slide: &Slide) -> Vec<String> {
    let mut out = Vec::new();
    for item in &slide.items {
        if item.kind.as_deref().is_some_and(|k| k != "text") {
            continue;
        }
        for line in &item.lines {
            let text: String = line.text.iter().map(|t| t.value.as_str()).collect();
            for part in text.split('\n') {
                out.push(part.trim_end().to_string());
            }
            if line.chords.is_empty() || text.contains('\n') {
                continue;
            }
            // Re-insert chords into the line just pushed, from the end.
            let last = out.last_mut().unwrap();
            let mut chars: Vec<char> = last.chars().collect();
            let mut chords: Vec<&Chord> = line.chords.iter().collect();
            chords.sort_by_key(|c| std::cmp::Reverse(c.pos));
            let orig_len = text.chars().count();
            for c in chords {
                let pos = c.pos.min(chars.len().min(orig_len));
                for (i, ch) in format!("[{}]", c.key).chars().enumerate() {
                    chars.insert(pos + i, ch);
                }
            }
            *last = chars.into_iter().collect();
        }
    }
    while out.last().is_some_and(|l| l.trim().is_empty()) {
        out.pop();
    }
    while out.first().is_some_and(|l| l.trim().is_empty()) {
        out.remove(0);
    }
    out
}

/// Kind key: the global group, else a guess from the group name, else the
/// custom group name itself.
fn group_key(slide: &Slide) -> String {
    let global = slide
        .global_group
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_lowercase();
    if let Some(k) = known(&global) {
        return k.into();
    }
    let group = slide.group.as_deref().unwrap_or("").trim().to_lowercase();
    let stripped = group.trim_end_matches(|c: char| c.is_ascii_digit() || c == ' ');
    if let Some(k) = known(stripped) {
        return k.into();
    }
    if stripped.is_empty() {
        "verse".into()
    } else {
        stripped.to_string()
    }
}

fn known(name: &str) -> Option<&'static str> {
    Some(match name.replace(['-', ' '], "_").as_str() {
        "verse" => "verse",
        "pre_chorus" | "prechorus" => "pre_chorus",
        "chorus" | "refrain" => "chorus",
        "bridge" => "bridge",
        "tag" => "tag",
        "intro" => "intro",
        "outro" | "ending" => "outro",
        "break" => "break",
        _ => return None,
    })
}

fn label_base(key: &str) -> String {
    match key {
        "verse" => "V".into(),
        "pre_chorus" => "P".into(),
        "chorus" => "C".into(),
        "bridge" => "B".into(),
        "tag" => "T".into(),
        "intro" => "I".into(),
        "outro" => "E".into(),
        "break" => "Break".into(),
        other => {
            let mut c = other.chars();
            c.next()
                .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                .unwrap_or_default()
        }
    }
}
