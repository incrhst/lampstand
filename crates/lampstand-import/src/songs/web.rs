//! A song's web page: the lyrics and credits a publisher shows for one song,
//! such as `sovereigngracemusic.com/music/songs/<song>/`.
//!
//! There is no standard markup for lyrics, so the page is read in the order
//! most likely to be right:
//!
//! 1. schema.org `lyrics` in JSON-LD (`MusicComposition`, `MusicRecording`).
//! 2. An element whose class or id says "lyric" (`song-lyrics`,
//!    `#lyrics`): the innermost one with words in it.
//! 3. The content after a heading that reads "Lyrics", up to the next
//!    heading.
//!
//! The words are split into sections at headings ("VERSE 1", "[Chorus]")
//! and blank lines, as in a SongSelect lyrics export. A line of credits
//! ("Words and Music by…", "©…", "CCLI…") ends the lyrics; the author and
//! copyright are read from it. The title is the page's `og:title` or
//! `<title>` without the site's name, and the CCLI number is any "CCLI #"
//! on the page that is not a church's licence number.

use dom_query::{Document, NodeRef};
use lampstand_core::song::Song;

use super::songselect::{is_heading, trim_blank, SectionBuilder};

/// Reads the song on a page. The error says why there is none.
pub fn parse_page(html: &str) -> Result<Song, String> {
    let doc = Document::from(html);
    let (ld_title, blocks) = match json_ld_lyrics(&doc) {
        Some((title, text)) => (title, text_blocks(&text)),
        None => {
            let nodes = lyrics_by_name(&doc)
                .or_else(|| lyrics_after_heading(&doc))
                .ok_or("no lyrics found on this page")?;
            let mut blocks = Vec::new();
            for node in nodes {
                blocks.extend(node_blocks(&node));
            }
            (None, blocks)
        }
    };

    let title = ld_title
        .or_else(|| page_title(&doc))
        .ok_or("the page has no title")?;
    let (builder, footer) = sections(blocks);
    let mut song = builder.finish(Song::new(title));
    if song.sections.iter().map(|s| s.lines.len()).sum::<usize>() < 2 {
        return Err("no lyrics found on this page".into());
    }
    let credits = credits(&footer);
    song.author = credits.author;
    song.copyright = credits.copyright;
    song.ccli_number = credits.ccli.or_else(|| {
        let body = doc.select("body");
        let mut text = String::new();
        flatten(body.nodes().first()?, false, &mut text);
        ccli_number(&text)
    });
    Ok(song)
}

// ---------------------------------------------------------------- finding

/// `"lyrics": {"text": "…"}` (or a plain string) anywhere in the page's
/// JSON-LD, with the `name` beside it.
fn json_ld_lyrics(doc: &Document) -> Option<(Option<String>, String)> {
    fn find(value: &serde_json::Value) -> Option<(Option<String>, String)> {
        match value {
            serde_json::Value::Object(map) => {
                let text = match map.get("lyrics") {
                    Some(serde_json::Value::String(s)) => Some(s.clone()),
                    Some(serde_json::Value::Object(l)) => {
                        l.get("text").and_then(|t| t.as_str()).map(str::to_string)
                    }
                    _ => None,
                };
                if let Some(text) = text.filter(|t| !t.trim().is_empty()) {
                    let name = map
                        .get("name")
                        .and_then(|n| n.as_str())
                        .map(|n| n.trim().to_string())
                        .filter(|n| !n.is_empty());
                    return Some((name, text));
                }
                map.values().find_map(find)
            }
            serde_json::Value::Array(items) => items.iter().find_map(find),
            _ => None,
        }
    }
    doc.select(r#"script[type="application/ld+json"]"#)
        .iter()
        .filter_map(|s| serde_json::from_str::<serde_json::Value>(&s.text()).ok())
        .find_map(|v| find(&v))
}

/// The innermost elements whose class or id has "lyric" in it and words
/// under it: the longest of them and any others of its class, with the
/// credits after them if any.
fn lyrics_by_name<'a>(doc: &'a Document) -> Option<Vec<NodeRef<'a>>> {
    let named = |n: &NodeRef| {
        n.is_element()
            && !matches!(name(n).as_str(), "html" | "body" | "a" | "button")
            && [n.class(), n.id_attr()]
                .into_iter()
                .flatten()
                .any(|v| v.to_lowercase().contains("lyric"))
    };
    let body = doc.select("body");
    let root = body.nodes().first()?;
    let candidates: Vec<NodeRef> = root
        .descendants_it()
        .filter(|n| named(n) && lyric_lines(n) >= 2)
        .collect();
    let innermost = candidates.iter().filter(|c| {
        !candidates
            .iter()
            .any(|o| o.id != c.id && o.ancestors_it(None).any(|a| a.id == c.id))
    });
    let innermost: Vec<&NodeRef> = innermost.collect();
    let best = *(*innermost.iter().max_by_key(|n| lyric_lines(n))?);
    // Genius splits the words over several containers of the same class.
    let mut nodes: Vec<NodeRef> = innermost
        .iter()
        .filter(|n| n.id == best.id || (n.class().is_some() && n.class() == best.class()))
        .map(|n| *(*n))
        .collect();
    let last = *nodes.last()?;
    if let Some(next) = last.next_element_sibling() {
        if node_blocks(&next)
            .first()
            .and_then(|b| b.first())
            .is_some_and(|l| is_credit(l))
        {
            nodes.push(next);
        }
    }
    Some(nodes)
}

/// The elements after a "Lyrics" heading (or tab), climbing out of the
/// wrappers a page builder puts around it, up to the next heading.
fn lyrics_after_heading<'a>(doc: &'a Document) -> Option<Vec<NodeRef<'a>>> {
    let body = doc.select("body");
    let root = body.nodes().first()?;
    let heading = root.descendants_it().find(|n| {
        matches!(
            name(n).as_str(),
            "h1" | "h2"
                | "h3"
                | "h4"
                | "h5"
                | "h6"
                | "strong"
                | "b"
                | "p"
                | "span"
                | "dt"
                | "button"
                | "li"
                | "a"
        ) && is_lyrics_title(&n.text())
    })?;
    let mut at = heading;
    for _ in 0..6 {
        let mut nodes = Vec::new();
        let mut next = at.next_element_sibling();
        while let Some(n) = next {
            next = n.next_element_sibling();
            // "Verse 1" as a heading of its own is part of the words; any
            // other heading starts the next part of the page.
            if section_heading(n.text().trim()).is_some() || !is_heading_element(&n) {
                nodes.push(n);
            } else if is_heading_tag(&n) {
                break;
            } else if nodes.is_empty() {
                // A panel of words with headings inside it (WorshipOnline's
                // Lyrics tab).
                nodes.push(n);
            } else {
                break;
            }
        }
        if nodes.iter().map(lyric_lines).sum::<usize>() >= 2 {
            return Some(nodes);
        }
        at = at.parent().filter(|p| name(p) != "body")?;
    }
    None
}

fn is_lyrics_title(text: &str) -> bool {
    let t = text.trim().trim_end_matches(':').trim().to_lowercase();
    t == "lyrics" || t == "song lyrics"
}

fn is_heading_tag(n: &NodeRef) -> bool {
    matches!(name(n).as_str(), "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
}

/// A heading, or an element with one in it.
fn is_heading_element(n: &NodeRef) -> bool {
    is_heading_tag(n) || n.descendants_it().any(|d| is_heading_tag(&d))
}

fn lyric_lines(n: &NodeRef) -> usize {
    node_blocks(n).iter().map(Vec::len).sum()
}

fn name(n: &NodeRef) -> String {
    n.node_name().map(|t| t.to_lowercase()).unwrap_or_default()
}

// ---------------------------------------------------------------- text

const SKIPPED: &[&str] = &[
    "script", "style", "noscript", "template", "svg", "button", "form", "select", "textarea",
    "iframe", "nav", "head",
];

const BLOCKS: &[&str] = &[
    "p",
    "div",
    "section",
    "article",
    "header",
    "footer",
    "main",
    "aside",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "li",
    "ul",
    "ol",
    "dl",
    "dt",
    "dd",
    "blockquote",
    "pre",
    "table",
    "tr",
    "td",
    "th",
    "figure",
    "hr",
];

/// An element's words as blocks of lines: `<br>` ends a line, a paragraph
/// or other block element ends a block. A page that keeps its line breaks
/// in the text (`<pre>`, or CSS `white-space: pre-line`) is read as text.
fn node_blocks(node: &NodeRef) -> Vec<Vec<String>> {
    let mut out = String::new();
    flatten(node, false, &mut out);
    let blocks = text_blocks(&out);
    if blocks.iter().map(Vec::len).sum::<usize>() < 3 {
        let mut kept = String::new();
        flatten(node, true, &mut kept);
        let kept = text_blocks(&kept);
        if kept.iter().map(Vec::len).sum::<usize>() > blocks.iter().map(Vec::len).sum() {
            return kept;
        }
    }
    blocks
}

fn flatten(node: &NodeRef, keep_newlines: bool, out: &mut String) {
    if node.is_text() {
        let text = node.text();
        if keep_newlines {
            out.push_str(&text);
        } else {
            let mut space = false;
            for c in text.chars() {
                if c.is_whitespace() {
                    space = true;
                } else {
                    if space {
                        out.push(' ');
                        space = false;
                    }
                    out.push(c);
                }
            }
            if space {
                out.push(' ');
            }
        }
        return;
    }
    if !node.is_element() && !node.is_document() && !node.is_fragment() {
        return;
    }
    let tag = name(node);
    // Genius marks the header and bio inside its lyrics this way.
    if SKIPPED.contains(&tag.as_str())
        || node.attr("data-exclude-from-selection").as_deref() == Some("true")
    {
        return;
    }
    if tag == "br" {
        out.push('\n');
        return;
    }
    let block = BLOCKS.contains(&tag.as_str());
    if block {
        out.push_str("\n\n");
    }
    let keep = keep_newlines || tag == "pre";
    for child in node.children_it(false) {
        flatten(&child, keep, out);
    }
    if block {
        out.push_str("\n\n");
    }
}

/// Plain text as blocks of trimmed lines, split at blank lines.
fn text_blocks(text: &str) -> Vec<Vec<String>> {
    let text = if text.contains('<') && text.contains('>') {
        // JSON-LD lyrics are sometimes HTML.
        let doc = Document::fragment(text);
        let mut out = String::new();
        flatten(&doc.root(), false, &mut out);
        out
    } else {
        text.to_string()
    };
    let mut blocks = Vec::new();
    let mut block = Vec::new();
    for line in text.lines() {
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if line.is_empty() {
            if !block.is_empty() {
                blocks.push(std::mem::take(&mut block));
            }
        } else {
            block.push(line);
        }
    }
    if !block.is_empty() {
        blocks.push(block);
    }
    blocks
}

// ---------------------------------------------------------------- sections

/// The song's sections, and the lines from the first credit on.
fn sections(blocks: Vec<Vec<String>>) -> (SectionBuilder, Vec<String>) {
    let mut builder = SectionBuilder::default();
    let mut heading: Option<String> = None;
    let mut lines: Vec<String> = Vec::new();
    let flush =
        |builder: &mut SectionBuilder, heading: &mut Option<String>, lines: &mut Vec<String>| {
            let h = heading.take();
            let taken = trim_blank(std::mem::take(lines));
            if taken.is_empty() {
                if let Some(h) = h {
                    builder.repeat(&h);
                }
            } else {
                builder.push(h.as_deref().unwrap_or(""), taken);
            }
        };
    let mut footer = Vec::new();
    let mut all = blocks.into_iter();
    'blocks: for block in all.by_ref() {
        let mut block = block.into_iter();
        while let Some(line) = block.next() {
            if is_credit(&line) {
                footer.push(line);
                footer.extend(block);
                break 'blocks;
            }
            if is_lyrics_title(&line) {
                continue;
            }
            if let Some(h) = section_heading(&line) {
                if heading.is_some() || !lines.is_empty() {
                    flush(&mut builder, &mut heading, &mut lines);
                }
                heading = Some(h);
                continue;
            }
            lines.push(line);
        }
        // A heading on its own (`<h4>Chorus</h4>`) heads the next block.
        if !lines.is_empty() {
            flush(&mut builder, &mut heading, &mut lines);
        }
    }
    footer.extend(all.flatten());
    flush(&mut builder, &mut heading, &mut lines);
    (builder, footer)
}

/// "VERSE 1", "Chorus:", "[Bridge]", "(Pre-Chorus)".
///
/// A whole line in square brackets is a heading whatever it says, as on
/// Genius ("[Post-Chorus]"), less any singer after a colon ("[Verse 1:
/// Matt Redman]").
fn section_heading(line: &str) -> Option<String> {
    let line = line.trim();
    if let Some(inner) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
        let inner = inner.split(':').next().unwrap_or(inner).trim();
        if !inner.is_empty() && !inner.contains(['[', ']']) {
            return Some(inner.to_string());
        }
    }
    let bare = line
        .trim_start_matches(['[', '('])
        .trim_end_matches([']', ')'])
        .trim();
    is_heading(bare).then(|| bare.to_string())
}

// ---------------------------------------------------------------- credits

const CREDIT_STARTS: &[&str] = &[
    "©",
    "(c)",
    "copyright",
    "ccli",
    "words and music",
    "music and words",
    "words & music",
    "music & words",
    "words by",
    "music by",
    "lyrics by",
    "written by",
    "songwriters",
];

fn is_credit(line: &str) -> bool {
    let lower = line.trim().to_lowercase();
    lower.contains('©') || CREDIT_STARTS.iter().any(|s| lower.starts_with(s))
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Credits {
    author: Option<String>,
    copyright: Option<String>,
    ccli: Option<u32>,
}

/// "Music and Words by A, B, and C © 2017 Publisher. All rights reserved."
fn credits(lines: &[String]) -> Credits {
    let mut out = Credits::default();
    for line in lines.iter().take(6) {
        let line = line.trim();
        let lower = line.to_lowercase();
        if out.ccli.is_none() && lower.starts_with("ccli") {
            out.ccli = ccli_number(line);
            continue;
        }
        let (names, rest) = match line.find('©') {
            Some(at) => (&line[..at], Some(&line[at..])),
            None if lower.starts_with("copyright") => ("", Some(line)),
            None => (line, None),
        };
        if out.author.is_none() {
            out.author = authors(names);
        }
        if out.copyright.is_none() {
            out.copyright = rest.map(|r| r.trim().to_string()).filter(|r| r.len() > 1);
        }
    }
    out
}

/// The names after "…by", joined with commas.
fn authors(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    let at = lower.find(" by ").map(|i| i + 4).or_else(|| {
        lower
            .starts_with("songwriters")
            .then(|| lower.find(':').map(|i| i + 1))
            .flatten()
    })?;
    let names = text[at..]
        .replace(", and ", ", ")
        .replace(" and ", ", ")
        .replace(" & ", ", ")
        .replace(" / ", ", ");
    let names: Vec<&str> = names
        .split(',')
        .map(|n| n.trim().trim_end_matches(['.', ';']).trim())
        .filter(|n| !n.is_empty())
        .collect();
    (!names.is_empty()).then(|| names.join(", "))
}

/// The first "CCLI #1234567", "CCLI Song # 1234567" or "CCLI: 1234567"
/// in `text` ("CCLI" and the number may be in neighbouring cells),
/// skipping a church's "CCLI License #".
fn ccli_number(text: &str) -> Option<u32> {
    let lower = text.to_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find("ccli") {
        let start = from + i + 4;
        from = start;
        let gap: String = lower[start..]
            .chars()
            .take_while(|c| !c.is_ascii_digit())
            .take(17)
            .collect();
        if gap.chars().count() > 16 || gap.contains("licen") {
            continue;
        }
        let digits: String = lower[start + gap.len()..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if (3..=8).contains(&digits.len()) {
            return digits.parse().ok();
        }
    }
    None
}

// ---------------------------------------------------------------- title

/// `og:title` or `<title>`, without the site's name ("Song | Site",
/// "Song - Site"), a "Lyrics" tag, or the artist when a `<h1>` names the
/// song alone; the page's only `<h1>` failing those.
fn page_title(doc: &Document) -> Option<String> {
    let site = doc
        .select(r#"meta[property="og:site_name"]"#)
        .attr("content")
        .map(|s| s.trim().to_string());
    let raw = doc
        .select(r#"meta[property="og:title"]"#)
        .attr("content")
        .map(|t| t.to_string())
        .filter(|t| !t.trim().is_empty())
        .or_else(|| Some(doc.select("title").text().to_string()))
        .filter(|t| !t.trim().is_empty());
    let cleaned = raw.map(|raw| {
        let mut t = raw.split(" | ").next().unwrap_or(&raw).trim().to_string();
        if let Some(site) = site.as_deref().filter(|s| !s.is_empty()) {
            for sep in [" - ", " – ", " — ", " :: "] {
                if let Some(stripped) = t.strip_suffix(&format!("{sep}{site}")) {
                    t = stripped.trim().to_string();
                }
            }
        }
        let lower = t.to_lowercase();
        if lower.ends_with(" lyrics") {
            t.truncate(t.len() - " lyrics".len());
        } else if lower.starts_with("lyrics: ") {
            t = t["lyrics: ".len()..].to_string();
        }
        t.trim()
            .trim_end_matches(['-', '–', '—', ':', '|'])
            .trim()
            .to_string()
    });
    let h1s: Vec<String> = doc
        .select("h1")
        .iter()
        .map(|h| h.text().split_whitespace().collect::<Vec<_>>().join(" "))
        .collect();
    // "Title - Artist" or "Artist – Title": the half the page's heading names.
    let cleaned = cleaned.map(|t| {
        for sep in [" - ", " – ", " — "] {
            if let Some((a, b)) = t.split_once(sep) {
                for part in [a.trim(), b.trim()] {
                    if h1s.iter().any(|h| h.eq_ignore_ascii_case(part)) {
                        return part.to_string();
                    }
                }
            }
        }
        t
    });
    cleaned.filter(|t| !t.is_empty()).or_else(|| {
        let h1 = doc.select("h1");
        (h1.length() == 1)
            .then(|| h1.text().trim().to_string())
            .filter(|t| !t.is_empty())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(song: &Song, label: &str) -> Vec<String> {
        song.section(label).unwrap().lines.clone()
    }

    /// The shape of a Sovereign Grace Music song page (an Elementor
    /// WordPress site): the "Lyrics" heading in its own widget, the words
    /// in the next, the credits after them. Placeholder words.
    const SGM: &str = r#"<!doctype html><html><head>
        <title>Our Daily Song | Sovereign Grace Music</title>
        <meta property="og:title" content="Our Daily Song | Sovereign Grace Music">
        <meta property="og:site_name" content="Sovereign Grace Music">
        </head><body>
        <nav><a href="/">Music</a> Lyrics</nav>
        <h1>Our Daily Song</h1>
        <div>CCLI: #1234567</div>
        <div class="elementor-widget-heading"><div class="elementor-widget-container">
          <h3 class="elementor-heading-title">Lyrics</h3></div></div>
        <div class="elementor-widget-theme-post-content"><div class="elementor-widget-container">
          <p><strong>VERSE 1</strong><br />
          First line one,<br />
          First line two</p>
          <p><strong>CHORUS</strong><br />
          Chorus line one<br />
          Chorus line two</p>
          <p><strong>VERSE 2</strong><br />
          Second line one<br />
          Second line two</p>
          <p><strong>CHORUS</strong></p>
        </div></div>
        <div class="elementor-widget-text-editor"><div class="elementor-widget-container">
          Music and Words by Ann Writer, Ben Writer, and Cal Writer &copy; 2017 Some Praise/BMI (adm. by Someone). All rights reserved.
        </div></div>
        <div class="elementor-widget-button"><a href="x.pdf">Download Lyrics</a></div>
        <h3>Songwriters:</h3>
        <footer>&copy; Copyright 2026 Sovereign Grace Music. Church CCLI License # 99999</footer>
        </body></html>"#;

    #[test]
    fn reads_a_sovereign_grace_song_page() {
        let song = parse_page(SGM).unwrap();
        assert_eq!(song.title, "Our Daily Song");
        assert_eq!(lines(&song, "V1"), ["First line one,", "First line two"]);
        assert_eq!(lines(&song, "C"), ["Chorus line one", "Chorus line two"]);
        assert_eq!(lines(&song, "V2"), ["Second line one", "Second line two"]);
        assert_eq!(song.arrangement, ["V1", "C", "V2", "C"]);
        assert_eq!(
            song.author.as_deref(),
            Some("Ann Writer, Ben Writer, Cal Writer")
        );
        assert_eq!(
            song.copyright.as_deref(),
            Some("© 2017 Some Praise/BMI (adm. by Someone). All rights reserved.")
        );
        assert_eq!(song.ccli_number, Some(1234567));
    }

    #[test]
    fn reads_a_lyrics_element_with_headings_of_its_own() {
        let html = r#"<html><head><title>Be Still - Lyrics - Example Hymns</title>
            <meta property="og:site_name" content="Example Hymns"></head><body>
            <div class="song"><h2>Lyrics</h2>
              <div class="song-lyrics">
                <h4>[Verse 1]</h4><p>Line a<br>Line b</p>
                <h4>[Chorus]</h4><p>Line c<br>Line d</p>
              </div>
              <p class="credit">Words by Jane Hymnwriter (1850)</p>
            </div></body></html>"#;
        let song = parse_page(html).unwrap();
        assert_eq!(song.title, "Be Still");
        assert_eq!(lines(&song, "V1"), ["Line a", "Line b"]);
        assert_eq!(lines(&song, "C"), ["Line c", "Line d"]);
        assert!(song.arrangement.is_empty());
        assert_eq!(song.author.as_deref(), Some("Jane Hymnwriter (1850)"));
        assert_eq!(song.ccli_number, None);
    }

    #[test]
    fn numbers_unlabelled_verses_and_reads_pre_text() {
        let html = "<html><head><title>Plain Hymn Lyrics</title></head><body>
            <pre id=\"lyrics\">One, two
three, four

five, six
seven, eight</pre></body></html>";
        let song = parse_page(html).unwrap();
        assert_eq!(song.title, "Plain Hymn");
        assert_eq!(lines(&song, "V1"), ["One, two", "three, four"]);
        assert_eq!(lines(&song, "V2"), ["five, six", "seven, eight"]);
    }

    #[test]
    fn prefers_json_ld_lyrics() {
        let html = r#"<html><head><title>Ignored | Site</title>
            <script type="application/ld+json">{"@context":"https://schema.org","@graph":[
              {"@type":"WebPage"},
              {"@type":"MusicComposition","name":"Song Name",
               "lyrics":{"@type":"CreativeWork","text":"Verse 1\nA line\nB line\n\nChorus\nC line"}}]}
            </script></head><body><p>nothing</p></body></html>"#;
        let song = parse_page(html).unwrap();
        assert_eq!(song.title, "Song Name");
        assert_eq!(lines(&song, "V1"), ["A line", "B line"]);
        assert_eq!(lines(&song, "C"), ["C line"]);
    }

    /// Genius: the words split over several containers of one class, a
    /// header to leave out inside the first, `[Label: singer]` headings,
    /// and "Artist – Title" for a title.
    #[test]
    fn reads_a_genius_page() {
        let html = r##"<html><head><meta property="og:title" content="Some Artist – Song Name" />
            <meta property="og:site_name" content="Genius"/></head><body>
            <h1 class="SongHeader__Title"><span>Song Name</span></h1>
            <div data-lyrics-container="true" class="Lyrics__Container-sc-1 jhVhlB">
              <div data-exclude-from-selection="true" class="LyricsHeader__Container">
                <button>11 Contributors</button><h2>Song Name Lyrics</h2>
                <a href="#about">Song Name was written by…</a></div>
              [Verse 1: Some Artist]<br/>Line a<br/>Line b<br/><br/>[Chorus]<br/>Line c<br/>Line d</div>
            <div class="RightSidebar">ad</div>
            <div data-lyrics-container="true" class="Lyrics__Container-sc-1 jhVhlB">
              [Post-Chorus]<br/>Line e<br/>Line f<br/><br/>[Chorus]<br/>Line c<br/>Line d</div>
            <h1 class="PageFooterSocial__Slogan">Genius is the world's biggest collection</h1>
            </body></html>"##;
        let song = parse_page(html).unwrap();
        assert_eq!(song.title, "Song Name");
        assert_eq!(lines(&song, "V1"), ["Line a", "Line b"]);
        assert_eq!(lines(&song, "Post-Chorus"), ["Line e", "Line f"]);
        assert_eq!(song.arrangement, ["V1", "C", "Post-Chorus", "C"]);
    }

    /// WorshipOnline: a "Lyrics" tab button, the panel after the tabs with
    /// an `<h4>` per part, and the CCLI number in a table cell of its own.
    #[test]
    fn reads_a_worship_online_page() {
        let html = r#"<html><head><meta property="og:title" content="Song Name - Some Artist">
            </head><body><aside><h1>Song Name</h1>
            <table><tr><td>CCLI</td><td>3798438</td></tr></table></aside>
            <main><div class="tabs"><button>Charts</button><button>Tabs</button><button>Lyrics</button></div>
            <div class="panel"><div class="parts">
              <div><h4>Verse 1<!-- --> </h4><div class="part">Line a<br />Line b<br /></div></div>
              <div><h4>Chorus<!-- --> </h4><div class="part">Line c<br />Line d<br /></div></div>
            </div></div></main></body></html>"#;
        let song = parse_page(html).unwrap();
        assert_eq!(song.title, "Song Name");
        assert_eq!(lines(&song, "V1"), ["Line a", "Line b"]);
        assert_eq!(lines(&song, "C"), ["Line c", "Line d"]);
        assert_eq!(song.ccli_number, Some(3798438));
    }

    #[test]
    fn a_page_without_lyrics_is_an_error() {
        let html =
            "<html><head><title>About us</title></head><body><p>We are a church.</p></body></html>";
        assert_eq!(
            parse_page(html).unwrap_err(),
            "no lyrics found on this page"
        );
    }

    #[test]
    fn reads_ccli_numbers_but_not_licence_numbers() {
        assert_eq!(
            ccli_number("CCLI License # 11111 and CCLI Song # 7096689"),
            Some(7096689)
        );
        assert_eq!(ccli_number("CCLI: #7096689"), Some(7096689));
        assert_eq!(ccli_number("CCLI License #11111"), None);
    }
}
