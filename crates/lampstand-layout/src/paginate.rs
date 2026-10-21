//! Splitting songs and scripture into slides (T21).

use cosmic_text::{Attrs, Buffer, Family, Metrics, Shaping, Style, Weight};
use lampstand_core::output::{OutputConfig, OutputTarget};
use lampstand_core::program::SlideText;
use lampstand_core::scripture::ChapterVerse;
use lampstand_core::song::{lyrics, Section, Song};
use lampstand_core::theme::{TextStyle, Theme, REFERENCE_HEIGHT};

use crate::Fonts;

/// The scripture footer is set at this fraction of the body size, and a
/// gap of half a footer line separates it from the text. The compositor
/// (T23) must lay the footer out the same way.
pub const FOOTER_SCALE: f32 = 0.5;

/// Splits a song into slides: each arranged section becomes one or more
/// slides of at most `theme.max_lines` lines, split as evenly as possible
/// with longer slides first (at 4 lines: 6 is 3 + 3, 7 is 4 + 3, 9 is
/// 3 + 3 + 3). A blank line in a section forces a break. Chords are
/// stripped; `label` is the section label. The first slide carries the
/// attribution footer (author, copyright, CCLI number) when
/// `theme.show_copyright` is set.
///
/// `arrangement` replaces the song's own arrangement when given (the
/// per-service override); an empty one means the order the sections are
/// written in. A label that names no section is skipped rather than
/// reported: one stale label in a saved arrangement should not stop a song
/// going live. Sections with no lines produce no slides.
pub fn paginate_song(song: &Song, arrangement: Option<&[String]>, theme: &Theme) -> Vec<SlideText> {
    let order = arrangement.unwrap_or(&song.arrangement);
    let sections: Vec<&Section> = if order.is_empty() {
        song.sections.iter().collect()
    } else {
        order
            .iter()
            .filter_map(|label| song.sections.iter().find(|s| &s.label == label))
            .collect()
    };

    let max_lines = usize::from(theme.max_lines).max(1);
    let mut slides = Vec::new();
    for section in sections {
        for block in blocks(section) {
            for chunk in split_evenly(&block, max_lines) {
                slides.push(SlideText {
                    label: Some(section.label.clone()),
                    lines: chunk,
                    footer: None,
                });
            }
        }
    }
    if theme.show_copyright {
        if let Some(first) = slides.first_mut() {
            first.footer = attribution_line(song);
        }
    }
    slides
}

/// A section's lyric lines with chords removed, grouped by blank lines.
fn blocks(section: &Section) -> Vec<Vec<String>> {
    let mut blocks = Vec::new();
    let mut current = Vec::new();
    for line in &section.lines {
        let text = lyrics(line);
        if text.is_empty() {
            if !current.is_empty() {
                blocks.push(std::mem::take(&mut current));
            }
        } else {
            current.push(text);
        }
    }
    if !current.is_empty() {
        blocks.push(current);
    }
    blocks
}

/// Fewest slides of at most `max` lines, sizes as equal as possible with
/// the longer ones first.
fn split_evenly(lines: &[String], max: usize) -> Vec<Vec<String>> {
    let count = lines.len().div_ceil(max);
    let (base, extra) = (lines.len() / count, lines.len() % count);
    let mut start = 0;
    (0..count)
        .map(|i| {
            let len = base + usize::from(i < extra);
            let chunk = lines[start..start + len].to_vec();
            start += len;
            chunk
        })
        .collect()
}

/// The line under a song's first slide: who wrote it, who holds the
/// copyright, and its CCLI song number, whichever of them it has, joined by
/// " | ". The author is left out when the copyright line already names them
/// ("Words: John Newton"), so they are not credited twice.
fn attribution_line(song: &Song) -> Option<String> {
    let copyright = song
        .copyright
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let author = song
        .author
        .as_deref()
        .map(str::trim)
        .filter(|a| !a.is_empty())
        .filter(|a| !copyright.is_some_and(|c| c.to_lowercase().contains(&a.to_lowercase())));
    let ccli = song.ccli_number.map(|n| format!("CCLI Song #{n}"));
    let parts: Vec<&str> = [author, copyright, ccli.as_deref()]
        .into_iter()
        .flatten()
        .collect();
    (!parts.is_empty()).then(|| parts.join(" | "))
}

/// How a passage is shared out between slides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PassageSplit {
    /// Each verse on a slide of its own: the congregation reads along one
    /// verse at a time.
    #[default]
    Verse,
    /// As many whole verses as fit on each slide.
    Fill,
}

/// Splits a passage into slides that fit the primary output at no less
/// than the theme's preferred size where possible. [`PassageSplit::Verse`]
/// starts every verse on a new slide; [`PassageSplit::Fill`] breaks between
/// verses only when the next one does not fit. Either way a verse that does
/// not fit alone is split inside, at word boundaries. Verse numbers are kept as superscript-style prefixes ("16 For God
/// so loved...").
///
/// Each verse is one entry in `lines` ("16 For God..."), wrapped by the
/// renderer. `reference` is both the label and the footer of every slide,
/// so the footer's height (at [`FOOTER_SCALE`] of the body size, plus a gap
/// of half its line height) is taken off the text area before measuring.
pub fn paginate_passage(
    verses: &[(ChapterVerse, &str)],
    reference: &str,
    split: PassageSplit,
    theme: &Theme,
    primary: &OutputConfig,
    fonts: &mut Fonts,
) -> Vec<SlideText> {
    let measure = Measure::new(theme, primary, reference, fonts);
    let mut slides: Vec<Vec<String>> = Vec::new();
    let mut current: Vec<String> = Vec::new();

    for (cv, text) in verses {
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let numbered = format!("{} {}", cv.verse, text);

        if split == PassageSplit::Verse && !current.is_empty() {
            slides.push(std::mem::take(&mut current));
        }
        current.push(numbered.clone());
        if measure.fits(&current, fonts) {
            continue;
        }
        current.pop();
        if !current.is_empty() {
            slides.push(std::mem::take(&mut current));
        }
        current.push(numbered);
        if measure.fits(&current, fonts) {
            continue;
        }
        // One verse alone overflows: split it inside, at word boundaries.
        current.clear();
        let words: Vec<&str> = text.split_whitespace().collect();
        let mut rest = &words[..];
        let mut first = true;
        while !rest.is_empty() {
            let render = |n: usize| {
                let body = rest[..n].join(" ");
                if first {
                    format!("{} {}", cv.verse, body)
                } else {
                    body
                }
            };
            let n = measure.longest_prefix(rest.len(), render, fonts);
            let piece = render(n);
            rest = &rest[n..];
            first = false;
            if rest.is_empty() {
                // When filling, the tail may share a slide with the verses
                // that follow.
                current.push(piece);
            } else {
                slides.push(vec![piece]);
            }
        }
    }
    if !current.is_empty() {
        slides.push(current);
    }

    slides
        .into_iter()
        .map(|lines| SlideText {
            label: Some(reference.to_string()),
            lines,
            footer: Some(reference.to_string()),
        })
        .collect()
}

fn font_attrs(style: &TextStyle) -> Attrs<'_> {
    Attrs::new()
        .family(Family::Name(&style.font_family))
        .weight(Weight(style.weight))
        .style(if style.italic {
            Style::Italic
        } else {
            Style::Normal
        })
}

/// Whether text fits the primary output's text area at the theme's
/// preferred size scaled to the output's height.
struct Measure<'a> {
    style: &'a TextStyle,
    size: f32,
    /// Line height in pixels: the font's natural line height times
    /// `line_spacing`.
    line_height: f32,
    width: f32,
    /// Height left for text once the footer is reserved.
    height: f32,
}

impl<'a> Measure<'a> {
    fn new(theme: &'a Theme, primary: &OutputConfig, footer: &str, fonts: &mut Fonts) -> Self {
        let (w, h) = match &primary.target {
            OutputTarget::Display(d) => (d.width, d.height),
            OutputTarget::BrowserSource { width, height }
            | OutputTarget::Window { width, height } => (*width, *height),
        };
        let (w, h) = (w as f32, h as f32);
        let safe = primary.safe_area.rect(w, h);
        let area = theme.text_area;
        let size = theme.text.size * h / REFERENCE_HEIGHT;
        // Same rule as `fit_text` and the renderer: a multiple of the font
        // size. See `Paragraph::line_spacing`.
        let line_height = size * theme.text.line_spacing;
        let mut measure = Self {
            style: &theme.text,
            size,
            line_height,
            width: safe.width * area.width,
            height: safe.height * area.height,
        };
        measure.height -= measure.footer_height(footer, fonts);
        measure
    }

    /// Height taken by the footer and the gap above it.
    fn footer_height(&self, footer: &str, fonts: &mut Fonts) -> f32 {
        if footer.is_empty() {
            return 0.0;
        }
        let footer_line = self.line_height * FOOTER_SCALE;
        let rows = self.rows(
            &[footer.to_string()],
            self.size * FOOTER_SCALE,
            footer_line,
            fonts,
        );
        (rows as f32 + 0.5) * footer_line
    }

    /// Whether `lines`, wrapped to the area's width, fit its height.
    fn fits(&self, lines: &[String], fonts: &mut Fonts) -> bool {
        let rows = self.rows(lines, self.size, self.line_height, fonts);
        rows as f32 * self.line_height <= self.height
    }

    /// Rows `lines` wrap to at the given size, in the theme's font.
    fn rows(&self, lines: &[String], size: f32, line_height: f32, fonts: &mut Fonts) -> usize {
        let metrics = Metrics::new(size, line_height);
        let mut buffer = Buffer::new_empty(metrics);
        buffer.set_size(Some(self.width), None);
        let joined = lines.join("\n");
        let text = if self.style.uppercase {
            joined.to_uppercase()
        } else {
            joined
        };
        let attrs = font_attrs(self.style).metrics(metrics);
        buffer.set_text(&text, &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(fonts.system(), false);
        buffer.layout_runs().count()
    }

    /// Largest `n` in `1..=max` for which `line_for(n)` fits (1 when even
    /// one does not). Fitting is monotonic in `n`, so this bisects.
    fn longest_prefix(
        &self,
        max: usize,
        line_for: impl Fn(usize) -> String,
        fonts: &mut Fonts,
    ) -> usize {
        let (mut lo, mut hi) = (1, max);
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            if self.fits(&[line_for(mid)], fonts) {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        lo
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lampstand_core::geom::{Color, Rect};
    use lampstand_core::id::Id;
    use lampstand_core::output::{BackgroundMode, OutputRole};
    use lampstand_core::safe_area::SafeArea;
    use lampstand_core::scene::{HAlign, Transition, VAlign};
    use lampstand_core::theme::Background;

    fn raw_theme(max_lines: u8, show_copyright: bool) -> Theme {
        Theme {
            id: Id::generate(),
            name: "test".into(),
            text: TextStyle {
                font_family: "Noto Sans".into(),
                size: 100.0,
                min_size: 40.0,
                weight: 400,
                italic: false,
                color: Color::WHITE,
                align: HAlign::Center,
                valign: VAlign::Middle,
                line_spacing: 1.0,
                uppercase: false,
                shadow: None,
                outline: None,
            },
            background: Background::None,
            text_area: Rect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                // Set by `theme` to hold three lines and the footer.
                height: 1.0,
            },
            max_lines,
            transition: Transition::CUT,
            show_copyright,
        }
    }

    /// A theme whose text area holds exactly three lines plus the footer on
    /// a 1080-pixel-tall output.
    fn theme(max_lines: u8, show_copyright: bool) -> Theme {
        let mut theme = raw_theme(max_lines, show_copyright);
        let mut fonts = Fonts::bundled_only();
        let m = Measure::new(&theme, &output(1920, 1080), "John 3", &mut fonts);
        let reserved = 1080.0 - m.height; // footer, as the area is the full screen
        theme.text_area.height = (3.0 * m.line_height + reserved + 1.0) / 1080.0;
        theme
    }

    fn output(width: u32, height: u32) -> OutputConfig {
        OutputConfig {
            id: Id::generate(),
            name: "main".into(),
            role: OutputRole::Audience { primary: true },
            target: OutputTarget::Window { width, height },
            safe_area: SafeArea::default(),
            background: BackgroundMode::Bleed,
            theme: None,
            enabled: true,
        }
    }

    fn song(sections: &[(&str, &[&str])]) -> Song {
        let mut song = Song::new("Test");
        song.sections = sections
            .iter()
            .map(|(label, lines)| Section {
                label: (*label).into(),
                lines: lines.iter().map(|l| (*l).into()).collect(),
            })
            .collect();
        song
    }

    fn numbered(n: usize) -> Vec<String> {
        (1..=n).map(|i| format!("line {i}")).collect()
    }

    fn sizes(slides: &[SlideText]) -> Vec<usize> {
        slides.iter().map(|s| s.lines.len()).collect()
    }

    fn split_sizes(lines: usize) -> Vec<usize> {
        let lines = numbered(lines);
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        sizes(&paginate_song(
            &song(&[("V1", &refs)]),
            None,
            &theme(4, false),
        ))
    }

    #[test]
    fn even_split_longer_slides_first() {
        assert_eq!(split_sizes(1), [1]);
        assert_eq!(split_sizes(4), [4]);
        assert_eq!(split_sizes(5), [3, 2]);
        assert_eq!(split_sizes(6), [3, 3]);
        assert_eq!(split_sizes(7), [4, 3]);
        assert_eq!(split_sizes(9), [3, 3, 3]);
        assert_eq!(split_sizes(10), [4, 3, 3]);
    }

    #[test]
    fn split_keeps_line_order_and_labels() {
        let slides = paginate_song(
            &song(&[("V1", &["a", "b", "c", "d", "e", "f", "g"])]),
            None,
            &theme(4, false),
        );
        assert_eq!(slides[0].lines, ["a", "b", "c", "d"]);
        assert_eq!(slides[1].lines, ["e", "f", "g"]);
        assert!(slides.iter().all(|s| s.label.as_deref() == Some("V1")));
    }

    #[test]
    fn chords_are_stripped() {
        let slides = paginate_song(
            &song(&[("V1", &["[G]Amazing [G7]grace", "how [C]sweet"])]),
            None,
            &theme(4, false),
        );
        assert_eq!(slides[0].lines, ["Amazing grace", "how sweet"]);
    }

    #[test]
    fn blank_line_forces_a_break() {
        let slides = paginate_song(
            &song(&[("V1", &["a", "b", "", "c", "d"])]),
            None,
            &theme(4, false),
        );
        assert_eq!(slides[0].lines, ["a", "b"]);
        assert_eq!(slides[1].lines, ["c", "d"]);
    }

    #[test]
    fn blank_lines_split_each_block_separately() {
        let slides = paginate_song(
            &song(&[("V1", &["a", "b", "c", "d", "e", "", "f"])]),
            None,
            &theme(4, false),
        );
        assert_eq!(sizes(&slides), [3, 2, 1]);
        // Doubled, leading, trailing and chord-only blank lines add nothing.
        let slides = paginate_song(
            &song(&[("V1", &["", "a", "", "", "[G]", "b", ""])]),
            None,
            &theme(4, false),
        );
        assert_eq!(sizes(&slides), [1, 1]);
    }

    #[test]
    fn empty_section_makes_no_slides() {
        let slides = paginate_song(&song(&[("V1", &[]), ("C", &["x"])]), None, &theme(4, false));
        assert_eq!(slides.len(), 1);
        assert_eq!(slides[0].label.as_deref(), Some("C"));
    }

    fn labels(slides: &[SlideText]) -> Vec<&str> {
        slides.iter().filter_map(|s| s.label.as_deref()).collect()
    }

    #[test]
    fn no_arrangement_uses_written_order() {
        let song = song(&[("V1", &["a"]), ("C", &["b"]), ("V2", &["c"])]);
        assert_eq!(
            labels(&paginate_song(&song, None, &theme(4, false))),
            ["V1", "C", "V2"]
        );
    }

    #[test]
    fn arrangement_with_repeats() {
        let mut song = song(&[("V1", &["a"]), ("C", &["b"]), ("V2", &["c"])]);
        song.arrangement = ["V1", "C", "V2", "C", "C"].map(String::from).to_vec();
        let slides = paginate_song(&song, None, &theme(4, false));
        assert_eq!(labels(&slides), ["V1", "C", "V2", "C", "C"]);
        assert_eq!(slides[3].lines, ["b"]);
    }

    #[test]
    fn service_override_beats_song_arrangement() {
        let mut song = song(&[("V1", &["a"]), ("C", &["b"])]);
        song.arrangement = ["V1", "C"].map(String::from).to_vec();
        let over = ["C".to_string(), "V1".to_string(), "C".to_string()];
        let slides = paginate_song(&song, Some(&over), &theme(4, false));
        assert_eq!(labels(&slides), ["C", "V1", "C"]);
    }

    #[test]
    fn unknown_label_is_skipped() {
        let mut song = song(&[("V1", &["a"]), ("C", &["b"])]);
        song.arrangement = ["V1", "Z9", "C"].map(String::from).to_vec();
        let slides = paginate_song(&song, None, &theme(4, false));
        assert_eq!(labels(&slides), ["V1", "C"]);
    }

    #[test]
    fn footer_on_first_slide_only() {
        let mut song = song(&[("V1", &["a", "b", "c", "d", "e"]), ("C", &["f"])]);
        song.copyright = Some("(c) 2001 Someone".into());
        song.ccli_number = Some(1234567);
        let slides = paginate_song(&song, None, &theme(4, true));
        assert_eq!(slides.len(), 3);
        assert_eq!(
            slides[0].footer.as_deref(),
            Some("(c) 2001 Someone | CCLI Song #1234567")
        );
        assert!(slides[1..].iter().all(|s| s.footer.is_none()));
    }

    #[test]
    fn the_author_leads_the_attribution_on_the_first_slide() {
        // A public-domain hymn: who wrote it, and that it is free to use.
        let mut hymn = song(&[("V1", &["a"])]);
        hymn.author = Some("John Newton".into());
        hymn.copyright = Some("Public Domain".into());
        assert_eq!(
            paginate_song(&hymn, None, &theme(4, true))[0]
                .footer
                .as_deref(),
            Some("John Newton | Public Domain")
        );
        // With everything: author, copyright, CCLI number.
        hymn.ccli_number = Some(4);
        assert_eq!(
            paginate_song(&hymn, None, &theme(4, true))[0]
                .footer
                .as_deref(),
            Some("John Newton | Public Domain | CCLI Song #4")
        );
    }

    #[test]
    fn a_song_with_only_an_author_still_credits_them() {
        let mut only = song(&[("V1", &["a"])]);
        only.author = Some("  Charles Wesley ".into());
        assert_eq!(
            paginate_song(&only, None, &theme(4, true))[0]
                .footer
                .as_deref(),
            Some("Charles Wesley")
        );
    }

    #[test]
    fn an_author_the_copyright_already_names_is_not_credited_twice() {
        let mut named = song(&[("V1", &["a"])]);
        named.author = Some("Chris Tomlin".into());
        named.copyright = Some("© 2004 Words and music by chris tomlin".into());
        assert_eq!(
            paginate_song(&named, None, &theme(4, true))[0]
                .footer
                .as_deref(),
            Some("© 2004 Words and music by chris tomlin")
        );
    }

    #[test]
    fn no_footer_when_theme_hides_it_or_song_has_none() {
        let mut with = song(&[("V1", &["a"])]);
        with.copyright = Some("(c) X".into());
        assert_eq!(paginate_song(&with, None, &theme(4, false))[0].footer, None);
        let without = song(&[("V1", &["a"])]);
        assert_eq!(
            paginate_song(&without, None, &theme(4, true))[0].footer,
            None
        );
    }

    // Scripture. The theme's text area holds three lines and the footer at 1080p.

    fn verse(n: u16) -> ChapterVerse {
        ChapterVerse {
            chapter: 3,
            verse: n,
        }
    }

    fn passage(texts: &[&str], out: &OutputConfig) -> Vec<SlideText> {
        passage_split(texts, PassageSplit::Fill, out)
    }

    fn passage_split(texts: &[&str], split: PassageSplit, out: &OutputConfig) -> Vec<SlideText> {
        let verses: Vec<(ChapterVerse, &str)> = texts
            .iter()
            .enumerate()
            .map(|(i, t)| (verse(i as u16 + 1), *t))
            .collect();
        let mut fonts = Fonts::bundled_only();
        paginate_passage(&verses, "John 3", split, &theme(4, false), out, &mut fonts)
    }

    #[test]
    fn passages_default_to_one_verse_per_slide() {
        assert_eq!(PassageSplit::default(), PassageSplit::Verse);
        let slides = passage_split(
            &["In the beginning", "Was the Word", "And the Word"],
            PassageSplit::Verse,
            &output(1920, 1080),
        );
        assert_eq!(sizes(&slides), [1, 1, 1]);
        assert_eq!(slides[1].lines, ["2 Was the Word"]);
        assert_eq!(slides[2].footer.as_deref(), Some("John 3"));
    }

    #[test]
    fn verse_split_still_breaks_an_over_long_verse() {
        let long = vec!["grace"; 120].join(" ");
        let slides = passage_split(
            &["Short", &long, "Last"],
            PassageSplit::Verse,
            &output(1920, 1080),
        );
        assert!(slides.len() > 3, "{} slides", slides.len());
        assert_eq!(slides[0].lines, ["1 Short"]);
        assert!(slides[1].lines[0].starts_with("2 grace"));
        // The long verse's tail does not share a slide with verse 3.
        assert_eq!(slides.last().unwrap().lines, ["3 Last"]);
        assert!(slides.iter().all(|s| s.lines.len() == 1));
    }

    #[test]
    fn passage_that_fits_is_one_slide() {
        let slides = passage(&["In the beginning", "Was the Word"], &output(1920, 1080));
        assert_eq!(slides.len(), 1);
        assert_eq!(slides[0].lines, ["1 In the beginning", "2 Was the Word"]);
        assert_eq!(slides[0].label.as_deref(), Some("John 3"));
        assert_eq!(slides[0].footer.as_deref(), Some("John 3"));
    }

    #[test]
    fn passage_breaks_between_verses() {
        let texts = ["Jesus wept"; 7];
        let slides = passage(&texts, &output(1920, 1080));
        assert_eq!(sizes(&slides), [3, 3, 1]);
        assert_eq!(slides[1].lines[0], "4 Jesus wept");
        assert_eq!(slides[2].lines, ["7 Jesus wept"]);
    }

    #[test]
    fn passage_pagination_scales_with_output_height() {
        let texts = ["Jesus wept"; 7];
        let hd = passage(&texts, &output(1920, 1080));
        assert_eq!(passage(&texts, &output(960, 540)), hd);
        assert_eq!(passage(&texts, &output(3840, 2160)), hd);
    }

    #[test]
    fn over_long_verse_splits_inside() {
        let long = vec!["grace"; 120].join(" ");
        let out = output(1920, 1080);
        let slides = passage(&["Short", &long, "Last"], &out);
        assert!(slides.len() > 2, "{} slides", slides.len());

        // "Short" stands alone before the long verse; the verse number
        // appears once, on the first piece.
        assert_eq!(slides[0].lines, ["1 Short"]);
        assert!(slides[1].lines[0].starts_with("2 grace"));
        let all: Vec<&String> = slides.iter().flat_map(|s| &s.lines).collect();
        assert_eq!(all.iter().filter(|l| l.starts_with("2 ")).count(), 1);

        // Every slide fits, and the words all survive in order.
        let th = theme(4, false);
        let m = Measure::new(&th, &out, "John 3", &mut Fonts::bundled_only());
        let mut fonts = Fonts::bundled_only();
        assert!(slides.iter().all(|s| m.fits(&s.lines, &mut fonts)));
        let words = all
            .iter()
            .flat_map(|l| l.split_whitespace())
            .filter(|w| *w == "grace")
            .count();
        assert_eq!(words, 120);

        // The last piece shares a slide with the verse that follows it.
        assert_eq!(slides.last().unwrap().lines.last().unwrap(), "3 Last");
    }

    #[test]
    fn empty_passage_has_no_slides() {
        assert!(passage(&[], &output(1920, 1080)).is_empty());
    }

    #[test]
    fn footer_space_is_reserved() {
        let th = theme(4, false);
        let out = output(1920, 1080);
        let mut fonts = Fonts::bundled_only();
        let with = Measure::new(&th, &out, "John 3", &mut fonts);
        let without = Measure::new(&th, &out, "", &mut fonts);
        // One footer row at half size plus half a footer line of gap.
        let expected = 1.5 * with.line_height * FOOTER_SCALE;
        assert!((without.height - with.height - expected).abs() < 0.01);
        // A fourth line no longer fits beside the footer.
        let four = ["Jesus wept"; 4].map(String::from);
        assert!(!with.fits(&four, &mut fonts));
        assert!(with.fits(&four[..3], &mut fonts));
    }
}
