//! DeckCraft PPTX reader fidelity harness (scratch, not part of Lampstand).
//!
//! For each .pptx: count what the package contains by scanning its XML directly (ground truth),
//! import it with `deckcraft_pptx::import_checked`, count what the model kept, and print one JSON
//! line per deck.
//!
//! Usage: deckcraft-eval FILE...

use std::collections::BTreeMap;
use std::io::{Cursor, Read};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Instant;

use deckcraft_model::anim::{AnimClass, AnimStart};
use deckcraft_model::{Presentation, Shape, ShapeKind};
use quick_xml::Reader;
use quick_xml::events::Event;
use serde_json::{Value, json};

#[derive(Default, Debug)]
struct Truth {
    producer: String,
    slides: usize,
    masters: usize,
    layouts: usize,
    text_chars: usize,
    shapes: usize,
    pictures: usize,
    videos: usize,
    audios: usize,
    transitions: usize,
    effects: BTreeMap<String, usize>,
    click_effects: usize,
    slide_clicks: Vec<usize>,
    notes_slides: usize,
    notes_chars: usize,
    external_media: usize,
}

fn zip_parts(bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let mut out = BTreeMap::new();
    for i in 0..z.len() {
        let mut f = z.by_index(i).map_err(|e| e.to_string())?;
        let name = f.name().to_string();
        if name.ends_with(".xml") || name.ends_with(".rels") {
            let mut v = vec![];
            f.read_to_end(&mut v).map_err(|e| e.to_string())?;
            out.insert(name, v);
        } else {
            out.insert(name, vec![]);
        }
    }
    Ok(out)
}

fn attr(e: &quick_xml::events::BytesStart, key: &[u8]) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == key)
        .map(|a| String::from_utf8_lossy(&a.value).into_owned())
}

fn non_ws(s: &str) -> usize {
    s.chars().filter(|c| !c.is_whitespace()).count()
}

/// Resolve the slide part names in presentation order.
fn slide_parts(parts: &BTreeMap<String, Vec<u8>>) -> Vec<String> {
    let rels = rels_of(parts, "ppt/_rels/presentation.xml.rels");
    let Some(pres) = parts.get("ppt/presentation.xml") else {
        return vec![];
    };
    let mut r = Reader::from_reader(pres.as_slice());
    let mut buf = vec![];
    let mut out = vec![];
    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.name().as_ref() == b"p:sldId" => {
                if let Some(id) = attr(&e, b"r:id")
                    && let Some((_, t)) = rels.get(&id)
                {
                    out.push(resolve("ppt/", t));
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    out
}

/// rId -> (type, target)
fn rels_of(parts: &BTreeMap<String, Vec<u8>>, name: &str) -> BTreeMap<String, (String, String)> {
    let mut out = BTreeMap::new();
    let Some(b) = parts.get(name) else { return out };
    let mut r = Reader::from_reader(b.as_slice());
    let mut buf = vec![];
    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if e.local_name().as_ref() == b"Relationship" =>
            {
                let id = attr(&e, b"Id").unwrap_or_default();
                let ty = attr(&e, b"Type").unwrap_or_default();
                let t = attr(&e, b"Target").unwrap_or_default();
                let ext = attr(&e, b"TargetMode").as_deref() == Some("External");
                out.insert(id, (ty, if ext { format!("external:{t}") } else { t }));
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    out
}

fn resolve(base_dir: &str, target: &str) -> String {
    if let Some(t) = target.strip_prefix('/') {
        return t.to_string();
    }
    let mut segs: Vec<&str> = base_dir
        .trim_end_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    for p in target.split('/') {
        match p {
            ".." => {
                segs.pop();
            }
            "." | "" => {}
            s => segs.push(s),
        }
    }
    segs.join("/")
}

fn rels_name(part: &str) -> String {
    let (dir, file) = part.rsplit_once('/').unwrap_or(("", part));
    format!("{dir}/_rels/{file}.rels")
}

#[derive(Default)]
struct SlideScan {
    text_chars: usize,
    body_text_chars: usize,
    shapes: usize,
    pictures: usize,
    videos: usize,
    audios: usize,
    transition: bool,
    effects: BTreeMap<String, usize>,
    clicks: usize,
    interactive_depth: Option<usize>,
}

fn scan_xml(b: &[u8]) -> SlideScan {
    let mut s = SlideScan::default();
    let mut r = Reader::from_reader(b);
    r.config_mut().trim_text(false);
    let mut buf = vec![];
    let mut stack: Vec<Vec<u8>> = vec![];
    let mut fallback_depth: Option<usize> = None;
    let mut in_t = false;
    // per-sp placeholder type stack (for notes body text)
    let mut sp_ph: Vec<Option<String>> = vec![];
    let mut pic_media: Vec<bool> = vec![];
    let shape_tags: [&[u8]; 6] = [
        b"p:sp",
        b"p:pic",
        b"p:graphicFrame",
        b"p:grpSp",
        b"p:cxnSp",
        b"p:contentPart",
    ];
    loop {
        let ev = r.read_event_into(&mut buf);
        let skipping = fallback_depth.is_some();
        match ev {
            Ok(Event::Start(e)) => {
                let name = e.name().as_ref().to_vec();
                if name == b"mc:Fallback" && fallback_depth.is_none() {
                    fallback_depth = Some(stack.len());
                }
                if !skipping
                    && name == b"p:cTn"
                    && attr(&e, b"nodeType").as_deref() == Some("interactiveSeq")
                    && s.interactive_depth.is_none()
                {
                    s.interactive_depth = Some(stack.len());
                }
                if !skipping && stack.last().is_some_and(|p| p == b"p:transition") {
                    s.transition = true;
                }
                if !skipping {
                    on_open(&e, &name, &mut s, &mut sp_ph, &mut pic_media, &shape_tags);
                    if name == b"a:t" {
                        in_t = true;
                    }
                }
                stack.push(name);
            }
            Ok(Event::Empty(e)) => {
                let name = e.name().as_ref().to_vec();
                if !skipping && stack.last().is_some_and(|p| p == b"p:transition") {
                    s.transition = true;
                }
                if !skipping {
                    on_open(&e, &name, &mut s, &mut sp_ph, &mut pic_media, &shape_tags);
                    on_close(&name, &mut s, &mut sp_ph, &mut pic_media);
                }
            }
            Ok(Event::End(_)) => {
                let name = stack.pop().unwrap_or_default();
                if s.interactive_depth.is_some_and(|d| stack.len() < d) {
                    s.interactive_depth = None;
                }
                if fallback_depth == Some(stack.len()) {
                    fallback_depth = None;
                } else if !skipping {
                    if name == b"a:t" {
                        in_t = false;
                    }
                    on_close(&name, &mut s, &mut sp_ph, &mut pic_media);
                }
            }
            Ok(Event::Text(t)) if in_t && !skipping => {
                let n = non_ws(&String::from_utf8_lossy(t.as_ref()));
                s.text_chars += n;
                if sp_ph
                    .iter()
                    .rev()
                    .flatten()
                    .next()
                    .is_some_and(|p| p == "body")
                {
                    s.body_text_chars += n;
                }
            }
            Ok(Event::GeneralRef(_)) if in_t && !skipping => {
                s.text_chars += 1;
                if sp_ph
                    .iter()
                    .rev()
                    .flatten()
                    .next()
                    .is_some_and(|p| p == "body")
                {
                    s.body_text_chars += 1;
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    s
}

fn on_open(
    e: &quick_xml::events::BytesStart,
    name: &[u8],
    s: &mut SlideScan,
    sp_ph: &mut Vec<Option<String>>,
    pic_media: &mut Vec<bool>,
    shape_tags: &[&[u8]],
) {
    if shape_tags.contains(&name) {
        s.shapes += 1;
    }
    match name {
        b"p:sp" => sp_ph.push(None),
        b"p:ph" => {
            if let Some(last) = sp_ph.last_mut() {
                *last = Some(attr(e, b"type").unwrap_or_else(|| "body".into()));
            }
        }
        b"p:pic" => pic_media.push(false),
        b"a:videoFile" | b"a:quickTimeFile" => {
            s.videos += 1;
            if let Some(l) = pic_media.last_mut() {
                *l = true;
            }
        }
        b"a:audioFile" | b"a:wavAudioFile" | b"a:audioCd" => {
            s.audios += 1;
            if let Some(l) = pic_media.last_mut() {
                *l = true;
            }
        }
        b"p:transition" => {
            if attr(e, b"advTm").is_some() {
                s.transition = true;
            }
        }
        b"p:cTn" => {
            if let Some(c) = attr(e, b"presetClass") {
                *s.effects.entry(c).or_default() += 1;
            }
            if attr(e, b"nodeType").as_deref() == Some("clickEffect")
                && s.interactive_depth.is_none()
            {
                s.clicks += 1;
            }
        }
        _ => {}
    }
}

fn on_close(
    name: &[u8],
    s: &mut SlideScan,
    sp_ph: &mut Vec<Option<String>>,
    pic_media: &mut Vec<bool>,
) {
    match name {
        b"p:sp" => {
            sp_ph.pop();
        }
        b"p:pic" => {
            if !pic_media.pop().unwrap_or(false) {
                s.pictures += 1;
            }
        }
        _ => {}
    }
}

fn truth(bytes: &[u8]) -> Result<Truth, String> {
    let parts = zip_parts(bytes)?;
    let mut t = Truth::default();
    if let Some(app) = parts.get("docProps/app.xml") {
        let s = String::from_utf8_lossy(app);
        let get = |tag: &str| {
            s.split(&format!("<{tag}>"))
                .nth(1)
                .and_then(|r| r.split('<').next())
                .unwrap_or("")
                .to_string()
        };
        t.producer = format!("{} {}", get("Application"), get("AppVersion"))
            .trim()
            .to_string();
    }
    t.masters = parts
        .keys()
        .filter(|k| k.starts_with("ppt/slideMasters/slideMaster") && k.ends_with(".xml"))
        .count();
    t.layouts = parts
        .keys()
        .filter(|k| k.starts_with("ppt/slideLayouts/slideLayout") && k.ends_with(".xml"))
        .count();
    let slides = slide_parts(&parts);
    t.slides = slides.len();
    for sp in &slides {
        let Some(b) = parts.get(sp) else { continue };
        let s = scan_xml(b);
        t.text_chars += s.text_chars;
        t.shapes += s.shapes;
        t.pictures += s.pictures;
        t.videos += s.videos;
        t.audios += s.audios;
        t.transitions += usize::from(s.transition);
        t.click_effects += s.clicks;
        t.slide_clicks.push(s.clicks);
        for (k, v) in s.effects {
            *t.effects.entry(k).or_default() += v;
        }
        let rels = rels_of(&parts, &rels_name(sp));
        let dir = sp.rsplit_once('/').map(|x| x.0).unwrap_or("");
        for (ty, target) in rels.values() {
            if (ty.ends_with("/video") || ty.ends_with("/audio") || ty.ends_with("/media"))
                && target.starts_with("external:")
            {
                t.external_media += 1;
            }
            if ty.ends_with("/notesSlide") {
                let np = resolve(dir, target);
                if let Some(nb) = parts.get(&np) {
                    let ns = scan_xml(nb);
                    if ns.body_text_chars > 0 {
                        t.notes_slides += 1;
                        t.notes_chars += ns.body_text_chars;
                    }
                }
            }
        }
    }
    Ok(t)
}

#[derive(Default, Debug)]
struct Model {
    text_chars: usize,
    shapes: usize,
    pictures: usize,
    videos: usize,
    audios: usize,
    opaque: usize,
    opaque_labels: BTreeMap<String, usize>,
}

fn walk(shapes: &[Shape], m: &mut Model) {
    for s in shapes {
        m.shapes += 1;
        if let Some(t) = &s.text {
            m.text_chars += non_ws(&t.text());
        }
        match &s.kind {
            ShapeKind::Picture { .. } => m.pictures += 1,
            ShapeKind::Media(c) => {
                if c.video {
                    m.videos += 1
                } else {
                    m.audios += 1
                }
            }
            ShapeKind::Group { children, .. } => walk(children, m),
            ShapeKind::Table(tb) => {
                for row in &tb.rows {
                    for c in &row.cells {
                        m.text_chars += non_ws(&c.text.text());
                    }
                }
            }
            ShapeKind::Opaque { label, .. } => {
                m.opaque += 1;
                *m.opaque_labels.entry(label.clone()).or_default() += 1;
            }
            _ => {}
        }
    }
}

fn model_counts(p: &Presentation) -> Value {
    let mut m = Model::default();
    let mut transitions = 0;
    let mut effects: BTreeMap<String, usize> = BTreeMap::new();
    let mut clicks = 0;
    let mut notes_slides = 0;
    let mut notes_chars = 0;
    let mut transition_kinds: BTreeMap<String, usize> = BTreeMap::new();
    let mut effect_ids: BTreeMap<String, usize> = BTreeMap::new();
    for s in &p.slides {
        walk(&s.shapes, &mut m);
        if let Some(t) = s
            .transition
            .as_ref()
            .filter(|t| t.kind != "none" || t.advance_after_ms.is_some())
        {
            transitions += 1;
            *transition_kinds.entry(t.kind.clone()).or_default() += 1;
        }
        for a in &s.animations {
            let c = match a.class {
                AnimClass::Entrance => "entr",
                AnimClass::Emphasis => "emph",
                AnimClass::Exit => "exit",
                AnimClass::Path => "path",
                AnimClass::Media => "mediacall",
            };
            *effects.entry(c.into()).or_default() += 1;
            *effect_ids.entry(format!("{c}:{}", a.effect)).or_default() += 1;
            if a.start == AnimStart::OnClick {
                clicks += 1;
            }
        }
        let n = non_ws(&s.notes_text());
        if n > 0 {
            notes_slides += 1;
            notes_chars += n;
        }
    }
    let media_missing = p
        .media
        .iter()
        .filter(|mi| mi.data.is_empty() && mi.link.is_none())
        .count();
    let media_linked = p.media.iter().filter(|mi| mi.link.is_some()).count();
    json!({
        "slides": p.slides.len(),
        "masters": p.masters.len(),
        "layouts": p.masters.iter().map(|m| m.layouts.len()).sum::<usize>(),
        "master_shapes": p.masters.iter().map(|m| m.shapes.len()).sum::<usize>(),
        "text_chars": m.text_chars,
        "shapes": m.shapes,
        "pictures": m.pictures,
        "videos": m.videos,
        "audios": m.audios,
        "opaque": m.opaque,
        "opaque_labels": m.opaque_labels,
        "transitions": transitions,
        "transition_kinds": transition_kinds,
        "effects": effects,
        "effect_ids": effect_ids,
        "click_effects": clicks,
        "slide_steps": (0..p.slides.len()).map(|i| { let tl = deckcraft_anim::ShowState::timeline(p, i); (0..tl.steps()).filter(|&k| !tl.step_is_auto(k)).count() }).collect::<Vec<_>>(),
        "notes_slides": notes_slides,
        "notes_chars": notes_chars,
        "media_items": p.media.len(),
        "media_missing_bytes": media_missing,
        "media_linked": media_linked,
    })
}

fn main() {
    // Keep the importer's own log quiet; we only report counts.
    std::panic::set_hook(Box::new(|_| {}));
    for path in std::env::args().skip(1) {
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                println!("{}", json!({"file": path, "error": e.to_string()}));
                continue;
            }
        };
        let t = truth(&bytes);
        let start = Instant::now();
        let res = catch_unwind(AssertUnwindSafe(|| deckcraft_pptx::import_checked(&bytes)));
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        let (status, model, skipped) = match res {
            Ok(Ok((p, skipped))) => ("ok", model_counts(&p), skipped),
            Ok(Err(e)) => ("error", json!({"error": e.to_string()}), vec![]),
            Err(_) => ("panic", Value::Null, vec![]),
        };
        let truth = match t {
            Ok(t) => json!({
                "producer": t.producer, "slides": t.slides, "masters": t.masters, "layouts": t.layouts,
                "text_chars": t.text_chars, "shapes": t.shapes, "pictures": t.pictures, "videos": t.videos,
                "audios": t.audios, "transitions": t.transitions, "effects": t.effects,
                "click_effects": t.click_effects, "slide_clicks": t.slide_clicks, "notes_slides": t.notes_slides, "notes_chars": t.notes_chars,
                "external_media": t.external_media,
            }),
            Err(e) => json!({"error": e}),
        };
        println!(
            "{}",
            json!({"file": path, "bytes": bytes.len(), "status": status, "import_ms": (ms * 10.0).round() / 10.0,
                   "skipped_slides": skipped, "truth": truth, "model": model})
        );
    }
}
