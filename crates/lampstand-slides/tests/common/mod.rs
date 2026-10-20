//! Building hostile decks from the synthetic ones.

#![allow(dead_code)]

use std::io::{Cursor, Read, Write};
use std::path::Path;

pub fn deck_bytes(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/decks")
        .join(format!("{name}.pptx"));
    std::fs::read(path).unwrap()
}

/// `name` rebuilt entry by entry: `f` gets each entry's name and bytes and
/// returns the entries to write in its place. Stored uncompressed.
pub fn rebuild(name: &str, mut f: impl FnMut(&str, Vec<u8>) -> Vec<(String, Vec<u8>)>) -> Vec<u8> {
    let mut zip = zip::ZipArchive::new(Cursor::new(deck_bytes(name))).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).unwrap();
        let file_name = file.name().unwrap().to_string();
        let mut data = Vec::new();
        file.read_to_end(&mut data).unwrap();
        for (n, d) in f(&file_name, data) {
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            out.start_file(n, options).unwrap();
            out.write_all(&d).unwrap();
        }
    }
    out.finish().unwrap().into_inner()
}

/// `name` with `junk` added to the end of the shape tree of each slide in
/// `slides` (1-based).
pub fn with_junk(name: &str, slides: &[usize], junk: &str) -> Vec<u8> {
    rebuild(name, |n, d| {
        let hit = slides
            .iter()
            .any(|k| n == format!("ppt/slides/slide{k}.xml"));
        if hit {
            let xml = String::from_utf8(d).unwrap();
            let xml = xml.replacen("</p:spTree>", &format!("{junk}</p:spTree>"), 1);
            vec![(n.to_string(), xml.into_bytes())]
        } else {
            vec![(n.to_string(), d)]
        }
    })
}

/// `name` with `copies` more copies of each listed slide (1-based) added
/// to the end of the deck: each copy its own part with its own
/// relationships (notes left out, as a real deck has its own per slide).
pub fn with_copies(name: &str, slides: &[(usize, usize)]) -> Vec<u8> {
    let mut zip = zip::ZipArchive::new(Cursor::new(deck_bytes(name))).unwrap();
    let mut read = |part: &str| {
        let mut s = String::new();
        zip.by_name(part).unwrap().read_to_string(&mut s).unwrap();
        s
    };
    let mut extra: Vec<(String, String, String)> = Vec::new();
    for (slide, copies) in slides {
        let xml = read(&format!("ppt/slides/slide{slide}.xml"));
        let rels = read(&format!("ppt/slides/_rels/slide{slide}.xml.rels"));
        let rels = rels
            .split("<Relationship ")
            .filter(|r| !r.contains("/notesSlide\""))
            .collect::<Vec<_>>()
            .join("<Relationship ");
        for k in 0..*copies {
            extra.push((format!("copy{slide}_{k}"), xml.clone(), rels.clone()));
        }
    }
    rebuild(name, |n, d| {
        let text = || String::from_utf8(d.clone()).unwrap();
        let mut out = Vec::new();
        match n {
            "ppt/presentation.xml" => {
                let ids: String = extra
                    .iter()
                    .enumerate()
                    .map(|(i, (stem, _, _))| {
                        format!(r#"<p:sldId id="{}" r:id="rId{stem}"/>"#, 1000 + i)
                    })
                    .collect();
                out.push((
                    n.to_string(),
                    text()
                        .replacen("</p:sldIdLst>", &format!("{ids}</p:sldIdLst>"), 1)
                        .into_bytes(),
                ));
            }
            "ppt/_rels/presentation.xml.rels" => {
                let rels: String = extra
                    .iter()
                    .map(|(stem, _, _)| format!(r#"<Relationship Id="rId{stem}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/{stem}.xml"/>"#))
                    .collect();
                out.push((
                    n.to_string(),
                    text()
                        .replacen("</Relationships>", &format!("{rels}</Relationships>"), 1)
                        .into_bytes(),
                ));
            }
            "[Content_Types].xml" => {
                let types: String = extra
                    .iter()
                    .map(|(stem, _, _)| format!(r#"<Override PartName="/ppt/slides/{stem}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>"#))
                    .collect();
                out.push((
                    n.to_string(),
                    text()
                        .replacen("</Types>", &format!("{types}</Types>"), 1)
                        .into_bytes(),
                ));
                for (stem, xml, rels) in &extra {
                    out.push((format!("ppt/slides/{stem}.xml"), xml.clone().into_bytes()));
                    out.push((
                        format!("ppt/slides/_rels/{stem}.xml.rels"),
                        rels.clone().into_bytes(),
                    ));
                }
            }
            _ => out.push((n.to_string(), d)),
        }
        out
    })
}

/// Deck 07 with each of its pictures replaced by `<!X` and `zeros` zero
/// bytes, deflated (T87's repro: the XML scan stops at once, and the
/// entry compresses to almost nothing). Written in chunks, so the zeros
/// are never held whole.
pub fn with_zero_media(zeros: usize) -> Vec<u8> {
    let mut zip = zip::ZipArchive::new(Cursor::new(deck_bytes("07-pictures-media"))).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .large_file(true);
    let chunk = vec![0_u8; 1 << 20];
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).unwrap();
        let name = file.name().unwrap().to_string();
        let mut data = Vec::new();
        file.read_to_end(&mut data).unwrap();
        out.start_file(name.as_str(), options).unwrap();
        if name.starts_with("ppt/media/image") {
            out.write_all(b"<!X").unwrap();
            let mut left = zeros;
            while left > 0 {
                let n = left.min(chunk.len());
                out.write_all(&chunk[..n]).unwrap();
                left -= n;
            }
        } else {
            out.write_all(&data).unwrap();
        }
    }
    out.finish().unwrap().into_inner()
}
