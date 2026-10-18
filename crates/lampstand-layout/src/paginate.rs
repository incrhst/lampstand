//! Splitting songs and scripture into slides (T21).

use lampstand_core::output::OutputConfig;
use lampstand_core::program::SlideText;
use lampstand_core::scripture::ChapterVerse;
use lampstand_core::song::Song;
use lampstand_core::theme::Theme;

use crate::Fonts;

/// Splits a song into slides: each arranged section becomes one or more
/// slides of at most `theme.max_lines` lines, split as evenly as possible
/// with longer slides first (at 4 lines: 6 is 3 + 3, 7 is 4 + 3, 9 is
/// 3 + 3 + 3). A blank line in a section forces a break. Chords are
/// stripped; `label` is the section label. The first slide carries the copyright footer when
/// `theme.show_copyright` is set.
pub fn paginate_song(song: &Song, arrangement: Option<&[String]>, theme: &Theme) -> Vec<SlideText> {
    let _ = (song, arrangement, theme);
    todo!("T21")
}

/// Splits a passage into slides that fit the primary output at no less
/// than the theme's preferred size where possible, breaking between
/// verses first and inside a verse only when one verse alone does not
/// fit. Verse numbers are kept as superscript-style prefixes ("16 For God
/// so loved...").
pub fn paginate_passage(
    verses: &[(ChapterVerse, &str)],
    reference: &str,
    theme: &Theme,
    primary: &OutputConfig,
    fonts: &mut Fonts,
) -> Vec<SlideText> {
    let _ = (verses, reference, theme, primary, fonts);
    todo!("T21")
}
