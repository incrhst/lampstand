//! Splitting a passage across slides by a line budget.
//!
//! PRD: BI-1 ("split long passages across slides automatically").

use lampstand_core::safe_area::Rect;

use crate::translation::Verse;

/// How much text one slide holds: lines of at most `chars_per_line`
/// characters, word-wrapped. The renderer derives this from the output's
/// safe area and font.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    pub chars_per_line: usize,
    pub lines: usize,
}

impl Budget {
    /// Estimate a budget for a text area given an average character width
    /// and line height in the same units (pixels, usually).
    pub fn from_rect(area: Rect, char_width: f32, line_height: f32) -> Budget {
        let fit = |len: f32, unit: f32| {
            if unit > 0.0 {
                (len / unit).floor().max(1.0) as usize
            } else {
                1
            }
        };
        Budget {
            chars_per_line: fit(area.width, char_width),
            lines: fit(area.height, line_height),
        }
    }
}

/// The part of one verse that sits on a slide.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlideVerse {
    pub chapter: u16,
    pub verse: u16,
    /// Verse number to show ("16", or "2:1" where a passage crosses into a
    /// new chapter). `None` when this piece continues a verse begun on the
    /// previous slide.
    pub label: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Slide {
    pub verses: Vec<SlideVerse>,
}

impl Slide {
    /// Plain text with verse numbers inline: "16 For God so loved…".
    pub fn text(&self) -> String {
        let mut out = String::new();
        for v in &self.verses {
            if !out.is_empty() {
                out.push(' ');
            }
            if let Some(label) = &v.label {
                out.push_str(label);
                out.push(' ');
            }
            out.push_str(&v.text);
        }
        out
    }

    /// Wrapped lines as the budget lays them out; what the splitter counted.
    pub fn lines(&self, chars_per_line: usize) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        for word in self.text().split_whitespace() {
            match lines.last_mut() {
                Some(line) if line.chars().count() + 1 + word.chars().count() <= chars_per_line => {
                    line.push(' ');
                    line.push_str(word);
                }
                _ => lines.push(word.to_string()),
            }
        }
        lines
    }
}

struct Piece<'a> {
    chapter: u16,
    verse: u16,
    label: Option<String>,
    words: Vec<&'a str>,
}

/// Lines needed to wrap words of these lengths greedily. A word longer than
/// a line gets a line to itself (it is never broken).
fn lines_for(lengths: impl IntoIterator<Item = usize>, width: usize) -> usize {
    let mut lines = 0;
    let mut line = 0;
    for len in lengths {
        if line > 0 && line + 1 + len <= width {
            line += 1 + len;
        } else {
            lines += 1;
            line = len;
        }
    }
    lines
}

fn piece_lengths<'p>(pieces: &'p [Piece<'_>]) -> impl Iterator<Item = usize> + 'p {
    pieces.iter().flat_map(|p| {
        p.label
            .iter()
            .map(|l| l.chars().count())
            .chain(p.words.iter().map(|w| w.chars().count()))
    })
}

fn ends_clause(word: &str) -> bool {
    word.trim_end_matches(['\u{2019}', '\u{201d}', '"', '\'', ')'])
        .ends_with(['.', ';', ':', ',', '!', '?'])
}

/// Split verses across slides so each slide's word-wrapped text fits the
/// budget.
///
/// - Words are never broken.
/// - A verse that fits on a slide of its own is never split: if it does not
///   fit after the verses already on the slide, it starts the next one.
/// - A verse longer than a whole slide is split between words, at the end
///   of a clause (after `.` `;` `:` `,` `!` `?`) when one falls in the back
///   half of the slide.
/// - Verse numbers are kept; a verse's continuation on the next slide has
///   no number.
pub fn split(verses: &[Verse], budget: Budget) -> Vec<Slide> {
    let width = budget.chars_per_line.max(1);
    let max_lines = budget.lines.max(1);
    let fits = |pieces: &[Piece<'_>], extra: &Piece<'_>| {
        lines_for(
            piece_lengths(pieces).chain(piece_lengths(std::slice::from_ref(extra))),
            width,
        ) <= max_lines
    };
    let crosses_chapters = verses.windows(2).any(|w| w[0].chapter != w[1].chapter);

    let mut slides = Vec::new();
    let mut current: Vec<Piece<'_>> = Vec::new();
    let flush = |current: &mut Vec<Piece<'_>>, slides: &mut Vec<Slide>| {
        if !current.is_empty() {
            let verses = current
                .drain(..)
                .map(|p| SlideVerse {
                    chapter: p.chapter,
                    verse: p.verse,
                    label: p.label,
                    text: p.words.join(" "),
                })
                .collect();
            slides.push(Slide { verses });
        }
    };

    let mut prev_chapter = None;
    for v in verses {
        let label = if crosses_chapters && prev_chapter != Some(v.chapter) {
            format!("{}:{}", v.chapter, v.verse)
        } else {
            v.verse.to_string()
        };
        prev_chapter = Some(v.chapter);
        let mut piece = Piece {
            chapter: v.chapter,
            verse: v.verse,
            label: Some(label),
            words: v.text.split_whitespace().collect(),
        };

        if fits(&current, &piece) {
            current.push(piece);
            continue;
        }
        let fits_alone = fits(&[], &piece);
        let current_lines = lines_for(piece_lengths(&current), width);
        if fits_alone || current_lines * 2 > max_lines {
            flush(&mut current, &mut slides);
        }
        if fits_alone {
            current.push(piece);
            continue;
        }

        // Longer than a slide: fill slides between words.
        loop {
            if fits(&current, &piece) {
                current.push(piece);
                break;
            }
            let mut take = 0;
            while take < piece.words.len() {
                let trial = Piece {
                    label: piece.label.clone(),
                    words: piece.words[..take + 1].to_vec(),
                    ..piece
                };
                if !fits(&current, &trial) {
                    break;
                }
                take += 1;
            }
            if take == 0 {
                if current.is_empty() {
                    // Not even one word fits an empty slide: give it its own.
                    take = 1;
                } else {
                    flush(&mut current, &mut slides);
                    continue;
                }
            }
            if let Some(clause) = (take.div_ceil(2)..take)
                .rev()
                .find(|&i| ends_clause(piece.words[i - 1]))
            {
                take = clause;
            }
            let rest = piece.words.split_off(take);
            current.push(Piece {
                label: piece.label.take(),
                ..piece
            });
            flush(&mut current, &mut slides);
            piece = Piece {
                chapter: v.chapter,
                verse: v.verse,
                label: None,
                words: rest,
            };
        }
    }
    flush(&mut current, &mut slides);
    slides
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::Book;

    fn verse(chapter: u16, verse: u16, text: &str) -> Verse {
        Verse {
            book: Book::JOHN,
            chapter,
            verse,
            text: text.into(),
        }
    }

    fn john_3() -> Vec<Verse> {
        vec![
            verse(3, 16, "For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life."),
            verse(3, 17, "For God sent not his Son into the world to condemn the world; but that the world through him might be saved."),
            verse(3, 18, "He that believeth on him is not condemned: but he that believeth not is condemned already, because he hath not believed in the name of the only begotten Son of God."),
        ]
    }

    fn assert_within(slides: &[Slide], budget: Budget) {
        for s in slides {
            let lines = s.lines(budget.chars_per_line);
            assert!(lines.len() <= budget.lines, "{lines:#?}");
            for l in lines {
                assert!(
                    l.chars().count() <= budget.chars_per_line || !l.contains(' '),
                    "{l}"
                );
            }
        }
    }

    fn rejoined(slides: &[Slide]) -> String {
        slides.iter().map(Slide::text).collect::<Vec<_>>().join(" ")
    }

    #[test]
    fn short_passage_is_one_slide() {
        let slides = split(
            &john_3(),
            Budget {
                chars_per_line: 60,
                lines: 12,
            },
        );
        assert_eq!(slides.len(), 1);
        assert!(slides[0].text().starts_with("16 For God so loved"));
        assert!(slides[0].text().contains(" 17 For God sent not"));
    }

    #[test]
    fn prefers_verse_boundaries() {
        let budget = Budget {
            chars_per_line: 40,
            lines: 5,
        };
        let slides = split(&john_3(), budget);
        assert_within(&slides, budget);
        // Every verse fits on a slide alone, so none is split.
        assert_eq!(slides.len(), 3);
        for (s, n) in slides.iter().zip(16..) {
            assert_eq!(s.verses.len(), 1);
            assert_eq!(s.verses[0].verse, n);
            assert_eq!(s.verses[0].label.as_deref(), Some(n.to_string().as_str()));
        }
    }

    #[test]
    fn long_verse_splits_between_words_at_a_clause() {
        let budget = Budget {
            chars_per_line: 30,
            lines: 3,
        };
        let slides = split(&john_3(), budget);
        assert_within(&slides, budget);
        assert_eq!(rejoined(&slides).split_whitespace().collect::<Vec<_>>(), {
            let full: Vec<String> = john_3()
                .iter()
                .map(|v| format!("{} {}", v.verse, v.text))
                .collect();
            full.join(" ")
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>()
        });
        // Verse 16 continues onto the next slide without repeating its number.
        assert_eq!(slides[0].verses[0].label.as_deref(), Some("16"));
        assert_eq!(slides[1].verses[0].verse, 16);
        assert_eq!(slides[1].verses[0].label, None);
        // The break falls after a clause.
        assert!(
            ends_clause(slides[0].text().split_whitespace().last().unwrap()),
            "{}",
            slides[0].text()
        );
    }

    #[test]
    fn chapter_labels_when_crossing_chapters() {
        let verses = vec![
            verse(1, 31, "And God saw every thing."),
            verse(2, 1, "Thus the heavens."),
        ];
        let slides = split(
            &verses,
            Budget {
                chars_per_line: 80,
                lines: 4,
            },
        );
        assert_eq!(
            slides[0].text(),
            "1:31 And God saw every thing. 2:1 Thus the heavens."
        );
    }

    #[test]
    fn word_longer_than_a_line_is_not_broken() {
        let verses = vec![verse(1, 1, "Mahershalalhashbaz is a long name")];
        let budget = Budget {
            chars_per_line: 8,
            lines: 2,
        };
        let slides = split(&verses, budget);
        assert!(slides
            .iter()
            .any(|s| s.text().contains("Mahershalalhashbaz")));
        assert_eq!(rejoined(&slides), "1 Mahershalalhashbaz is a long name");
        assert_within(&slides, budget);
    }

    #[test]
    fn budget_from_safe_area() {
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 1600.0,
            height: 900.0,
        };
        assert_eq!(
            Budget::from_rect(rect, 32.0, 100.0),
            Budget {
                chars_per_line: 50,
                lines: 9
            }
        );
    }
}
