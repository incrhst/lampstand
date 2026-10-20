//! What a deck may cost to parse, checked before DeckCraft reads it.
//!
//! DeckCraft builds a DOM of each XML part at about 1.3 KB per element and
//! allows up to 4 million elements and 96 MB per part, so a deck of a few
//! hundred KB (one slide with a 1000 × 400 table) can take 3.5 GB. These
//! limits keep the worst case near 2 GB. They sit at about five times the
//! largest real deck measured: the Apache POI slideshow corpus peaks at
//! 313,000 elements and 6.8 MB of XML in one deck, and 79,000 elements and
//! 1.7 MB in one part.
//!
//! XML parts are inflated here, streaming and only up to the limit, so a
//! zip directory that understates a part's size gains nothing.

use std::io::Read;

use crate::ImportError;

/// The sizes the zip directory declares, summed (media included).
pub(crate) const UNPACKED_BYTES: u64 = 1 << 30;
/// One XML part, inflated.
pub(crate) const PART_BYTES: u64 = 16 << 20;
/// Every XML part together, inflated.
pub(crate) const XML_BYTES: u64 = 64 << 20;
/// Elements in one XML part.
pub(crate) const PART_ELEMENTS: u64 = 500_000;
/// Elements in every XML part together.
pub(crate) const XML_ELEMENTS: u64 = 1_500_000;

fn too_large(what: String, found: u64, limit: u64) -> ImportError {
    ImportError::TooLarge { what, found, limit }
}

/// Refuses a deck that would cost more than the limits above to parse.
/// A package the zip reader can't open is left for DeckCraft to reject.
pub(crate) fn check(bytes: &[u8]) -> Result<(), ImportError> {
    let Ok(mut zip) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) else {
        return Ok(());
    };
    let declared = zip.decompressed_size().unwrap_or(0);
    if declared > u128::from(UNPACKED_BYTES) {
        let found = u64::try_from(declared).unwrap_or(u64::MAX);
        return Err(too_large("unpacked bytes".into(), found, UNPACKED_BYTES));
    }
    let (mut xml_bytes, mut xml_elements) = (0_u64, 0_u64);
    for i in 0..zip.len() {
        let Ok(part) = zip.by_index(i) else { continue };
        let name = part.name().map(|n| n.to_string()).unwrap_or_default();
        if !(name.ends_with(".xml") || name.ends_with(".rels")) {
            continue;
        }
        let (read, elements) = count(part.take(PART_BYTES + 1));
        if read > PART_BYTES {
            return Err(too_large(format!("bytes in {name}"), read, PART_BYTES));
        }
        if elements > PART_ELEMENTS {
            return Err(too_large(
                format!("XML elements in {name}"),
                elements,
                PART_ELEMENTS,
            ));
        }
        xml_bytes += read;
        xml_elements += elements;
        if xml_bytes > XML_BYTES {
            return Err(too_large("bytes of XML".into(), xml_bytes, XML_BYTES));
        }
        if xml_elements > XML_ELEMENTS {
            return Err(too_large("XML elements".into(), xml_elements, XML_ELEMENTS));
        }
    }
    Ok(())
}

/// Bytes read, and element start tags (`<` not followed by `/`, `?` or
/// `!`), streaming. A read error ends the count; DeckCraft reports it.
fn count(mut r: impl Read) -> (u64, u64) {
    let mut buf = [0_u8; 64 * 1024];
    let (mut read, mut elements) = (0_u64, 0_u64);
    // The previous chunk ended on `<`.
    let mut open = false;
    loop {
        let n = match r.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        read += n as u64;
        for &b in &buf[..n] {
            if open && !matches!(b, b'/' | b'?' | b'!') {
                elements += 1;
            }
            open = b == b'<';
        }
    }
    (read, elements)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_start_tags_across_chunks() {
        let xml = b"<?xml version=\"1.0\"?><!-- c --><a><b/><c>t</c></a>";
        assert_eq!(count(&xml[..]), (xml.len() as u64, 3));
        // A tag split across reads is counted once.
        let r = (&xml[..32]).chain(&xml[32..]);
        assert_eq!(count(r).1, 3);
    }
}
