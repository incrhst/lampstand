//! What a deck may cost to parse, checked before DeckCraft reads it.
//!
//! DeckCraft builds a DOM of each XML part at about 1.3 KB per element and
//! allows up to 4 million elements and 96 MB per part, so a deck of a few
//! hundred KB (one slide with a 1000 × 400 table) can take 3.5 GB.
//!
//! One budget covers the whole package, and every zip entry draws from it
//! whatever its name, extension or content type: DeckCraft finds parts by
//! relationship and lowercases their names, so a slide can be called
//! `slide1.XML` or `slide1.dat`, and its XML parser skips a BOM, decodes
//! UTF-16 and ignores text before the root element, so no prefix or
//! signature proves a part is not XML. So each entry is inflated as a
//! stream, its bytes counted against the deck and its element tags counted
//! (a `<`, a name, then white space, `/` or `>`; UTF-16 entries with their
//! zero bytes dropped). That test matches real XML exactly and compressed
//! media almost never: about 29,000 times per GiB of random bytes.
//!
//! The limits sit at about five times the largest real deck measured: the
//! Apache POI slideshow corpus peaks at 313,000 elements in a deck and
//! 79,000 in a part. At DeckCraft's 1.3 KB an element, parsing stays near
//! 2 GB at worst.

use std::io::Read;

use crate::ImportError;

/// What one deck may cost.
pub(crate) struct Limits {
    /// Bytes every entry inflates to, together (media included). Counted
    /// while reading, so a zip directory that understates sizes gains
    /// nothing.
    pub unpacked_bytes: u64,
    /// Element tags in one entry.
    pub part_elements: u64,
    /// Element tags in every entry together.
    pub deck_elements: u64,
}

pub(crate) const LIMITS: Limits = Limits {
    unpacked_bytes: 1 << 30,
    part_elements: 500_000,
    deck_elements: 1_500_000,
};

fn too_large(what: String, found: u64, limit: u64) -> ImportError {
    ImportError::TooLarge { what, found, limit }
}

/// Refuses a deck that would cost more than `LIMITS` to parse.
pub(crate) fn check(bytes: &[u8]) -> Result<(), ImportError> {
    check_with(bytes, &LIMITS)
}

fn check_with(bytes: &[u8], limits: &Limits) -> Result<(), ImportError> {
    // DeckCraft opened this package (`sniff`). If our zip reader can't, the
    // two disagree about what it holds, and it is not checked: refuse it.
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| ImportError::Corrupt(format!("unreadable package: {e}")))?;
    let declared = zip.decompressed_size().unwrap_or(0);
    if declared > u128::from(limits.unpacked_bytes) {
        let found = u64::try_from(declared).unwrap_or(u64::MAX);
        return Err(too_large(
            "unpacked bytes".into(),
            found,
            limits.unpacked_bytes,
        ));
    }
    let (mut unpacked, mut elements) = (0_u64, 0_u64);
    for i in 0..zip.len() {
        // An entry that can't be read (an unsupported compression method)
        // can't be read by DeckCraft either, which uses the same codecs.
        let Ok(entry) = zip.by_index(i) else { continue };
        let name = entry.name().map(|n| n.to_string()).unwrap_or_default();
        // Read at most one byte past what is left of the budget.
        let left = limits.unpacked_bytes - unpacked;
        let scan = scan(entry.take(left + 1));
        unpacked += scan.bytes;
        if unpacked > limits.unpacked_bytes {
            return Err(too_large(
                "unpacked bytes".into(),
                unpacked,
                limits.unpacked_bytes,
            ));
        }
        if scan.elements > limits.part_elements {
            return Err(too_large(
                format!("XML elements in {name}"),
                scan.elements,
                limits.part_elements,
            ));
        }
        elements += scan.elements;
        if elements > limits.deck_elements {
            return Err(too_large(
                "XML elements".into(),
                elements,
                limits.deck_elements,
            ));
        }
    }
    Ok(())
}

struct Scan {
    bytes: u64,
    elements: u64,
}

/// Where the tag test is in the input.
#[derive(Clone, Copy)]
enum State {
    Text,
    /// After `<`.
    Open,
    /// Inside a name that started with a name-start character.
    Name,
}

/// Counts bytes and element tags, streaming. A read error ends the scan;
/// DeckCraft reports the part.
fn scan(mut r: impl Read) -> Scan {
    let mut buf = [0_u8; 64 * 1024];
    let (mut bytes, mut elements) = (0_u64, 0_u64);
    let mut state = State::Text;
    // UTF-16 (decided by the entry's first two bytes): zero bytes dropped,
    // which leaves ASCII markup as it would be in UTF-8.
    let mut utf16 = None;
    loop {
        let n = match r.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        if utf16.is_none() {
            utf16 = Some(matches!(buf[..n.min(2)], [0xFF, 0xFE] | [0xFE, 0xFF]));
        }
        bytes += n as u64;
        for &b in &buf[..n] {
            if b == 0 && utf16 == Some(true) {
                continue;
            }
            state = match (state, b) {
                (_, b'<') => State::Open,
                (State::Open, b'A'..=b'Z' | b'a'..=b'z' | b'_') => State::Name,
                (
                    State::Name,
                    b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b':' | b'-',
                ) => State::Name,
                (State::Name, b' ' | b'\t' | b'\r' | b'\n' | b'/' | b'>') => {
                    elements += 1;
                    State::Text
                }
                _ => State::Text,
            };
        }
    }
    Scan { bytes, elements }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn elements(bytes: &[u8]) -> u64 {
        scan(bytes).elements
    }

    #[test]
    fn counts_start_tags_and_nothing_else() {
        let xml = b"<?xml version=\"1.0\"?><!-- c --><a:b c=\"1\"><d/><e>t</e></a:b>";
        assert_eq!(elements(xml), 3);
        // A tag split across reads is counted once.
        let r = (&xml[..35]).chain(&xml[35..]);
        assert_eq!(scan(r).elements, 3);
        assert_eq!(scan(&xml[..]).bytes, xml.len() as u64);
    }

    #[test]
    fn text_before_the_root_does_not_hide_xml() {
        // DeckCraft's parser ignores text before the root element.
        let mut part = b"\x89PNG\r\n\x1a\n not an image".to_vec();
        part.extend(b"<a:tc/>".repeat(1000));
        assert_eq!(elements(&part), 1000);
    }

    #[test]
    fn understated_and_oversized_entries_stay_within_the_budget() {
        use std::io::Write;
        let package = |declared: Option<u32>| {
            let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            out.start_file(
                "ppt/media/image1.png",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            out.write_all(&[7_u8; 64 << 10]).unwrap();
            let mut bytes = out.finish().unwrap().into_inner();
            if let Some(size) = declared {
                let central = bytes
                    .windows(4)
                    .position(|w| w == [0x50, 0x4b, 0x01, 0x02])
                    .unwrap();
                bytes[central + 24..central + 28].copy_from_slice(&size.to_le_bytes());
            }
            bytes
        };
        let small = Limits {
            unpacked_bytes: 1000,
            ..LIMITS
        };
        // Declared honestly: refused.
        assert!(matches!(
            check_with(&package(None), &small),
            Err(ImportError::TooLarge { .. })
        ));
        // Declared as 10 bytes: the zip reader yields at most the declared
        // size (and so does DeckCraft's, with the same codec), so the entry
        // costs 10 bytes to anyone who reads it.
        assert!(check_with(&package(Some(10)), &small).is_ok());
    }

    #[test]
    fn utf16_parts_are_counted() {
        let utf16: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain(
                "<p:sp><a:t>x</a:t></p:sp>"
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes),
            )
            .collect();
        assert_eq!(elements(&utf16), 2);
    }
}
