//! Splitting songs and scripture into slides (T21).

use cosmic_text::{Attrs, Buffer, Family, Metrics, Shaping, Style, Weight};
use lampstand_core::output::{OutputConfig, OutputTarget};
use lampstand_core::program::SlideText;
use lampstand_core::scripture::ChapterVerse;
use lampstand_core::song::{lyrics, Section, Song};
use lampstand_core::theme::{TextStyle, Theme, REFERENCE_HEIGHT};

use crate::Fonts;

/// Line height as a multiple of the font size when `line_spacing` is 1.
const LINE_HEIGHT_FACTOR: f32 = 1.2;

/// Splits a song into slides: each arranged section becomes one or more
/// slides of at most `theme.max_lines` lines, split as evenly as possible
/// with longer slides first (at 4 lines: 6 is 3 + 3, 7 is 4 + 3, 9 is
/// 3 + 3 + 3). A blank line in a section forces a break. Chords are
/// stripped; `label` is the section label. The first slide carries the copyright footer when
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
            first.footer = copyright_line(song);
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

fn copyright_line(song: &Song) -> Option<String> {
    let ccli = song.ccli_number.map(|n| format!("CCLI Song #{n}"));
    let parts: Vec<&str> = [song.copyright.as_deref(), ccli.as_deref()]
        .into_iter()
        .flatten()
        .filter(|s| !s.trim().is_empty())
        .collect();
    (!parts.is_empty()).then(|| parts.join(" | "))
}

/// Splits a passage into slides that fit the primary output at no less
/// than the theme's preferred size where possible, breaking between
/// verses first and inside a verse only when one verse alone does not
/// fit. Verse numbers are kept as superscript-style prefixes ("16 For God
/// so loved...").
///
/// Each verse is one entry in `lines` ("16 For God..."), wrapped by the
/// renderer. `reference` is both the label and the footer of every slide.
pub fn paginate_passage(
    verses: &[(ChapterVerse, &str)],
    reference: &str,
    theme: &Theme,
    primary: &OutputConfig,
    fonts: &mut Fonts,
) -> Vec<SlideText> {
    let measure = Measure::new(theme, primary);
    let mut slides: Vec<Vec<String>> = Vec::new();
    let mut current: Vec<String> = Vec::new();

    for (cv, text) in verses {
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let numbered = format!("{} {}", cv.verse, text);

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
                // The tail may share a slide with the verses that follow.
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

/// Whether text fits the primary output's text area at the theme's
/// preferred size scaled to the output's height.
struct Measure<'a> {
    style: &'a TextStyle,
    size: f32,
    width: f32,
    height: f32,
}

impl<'a> Measure<'a> {
    fn new(theme: &'a Theme, primary: &OutputConfig) -> Self {
        let (w, h) = match &primary.target {
            OutputTarget::Display(d) => (d.width, d.height),
            OutputTarget::BrowserSource { width, height }
            | OutputTarget::Window { width, height } => (*width, *height),
        };
        let (w, h) = (w as f32, h as f32);
        let safe = primary.safe_area.rect(w, h);
        let area = theme.text_area;
        Self {
            style: &theme.text,
            size: theme.text.size * h / REFERENCE_HEIGHT,
            width: safe.width * area.width,
            height: safe.height * area.height,
        }
    }

    /// Whether `lines`, wrapped to the area's width, fit its height.
    fn fits(&self, lines: &[String], fonts: &mut Fonts) -> bool {
        let line_height = self.size * LINE_HEIGHT_FACTOR * self.style.line_spacing;
        let metrics = Metrics::new(self.size, line_height);
        let mut buffer = Buffer::new_empty(metrics);
        buffer.set_size(Some(self.width), None);
        let joined = lines.join("\n");
        let text = if self.style.uppercase {
            joined.to_uppercase()
        } else {
            joined
        };
        let attrs = Attrs::new()
            .family(Family::Name(&self.style.font_family))
            .weight(Weight(self.style.weight))
            .style(if self.style.italic {
                Style::Italic
            } else {
                Style::Normal
            })
            .metrics(metrics);
        buffer.set_text(&text, &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(fonts.system(), false);
        let rows = buffer.layout_runs().count();
        rows as f32 * line_height <= self.height
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

    fn theme(max_lines: u8, show_copyright: bool) -> Theme {
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
                // Three 120 px lines on a 1080 px output.
                height: 0.34,
            },
            max_lines,
            transition: Transition::CUT,
            show_copyright,
        }
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

    // Scripture. The theme's text area holds three 120 px lines at 1080p.

    fn verse(n: u16) -> ChapterVerse {
        ChapterVerse {
            chapter: 3,
            verse: n,
        }
    }

    fn passage(texts: &[&str], out: &OutputConfig) -> Vec<SlideText> {
        let verses: Vec<(ChapterVerse, &str)> = texts
            .iter()
            .enumerate()
            .map(|(i, t)| (verse(i as u16 + 1), *t))
            .collect();
        let mut fonts = Fonts::bundled_only();
        paginate_passage(&verses, "John 3", &theme(4, false), out, &mut fonts)
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
        let m = Measure::new(&th, &out);
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
}
