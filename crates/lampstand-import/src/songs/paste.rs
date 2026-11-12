//! Lyrics pasted into the app: from a web page, an email, a chart or a
//! SongSelect lyrics page.
//!
//! Nothing here fetches anything; it reads the text it is given and makes a
//! [`Song`] for the person to check before it is saved.
//!
//! - **SongSelect's own layout** (a title, sections with headings and a
//!   `CCLI Song #` footer) is read exactly as the `.txt` export is.
//! - **Anything else** is split on blank lines. A block that starts with a
//!   heading (`Verse 1`, `[Chorus]`, `Chorus:`, `(Bridge)`, `V2`) is that
//!   section; blocks with no heading are numbered as verses. A heading with
//!   nothing under it, written again, is that section sung again (a
//!   repeated chorus), so the song gets an arrangement.
//! - **A title** is a first block of a single line that is not a heading,
//!   when more text follows. A title the person typed wins, and the same
//!   line pasted at the top is not taken for a lyric.
//! - **Chords** written above the words (a line of nothing but chords) are
//!   left out, and the result says how many lines were. ChordPro chords
//!   inside a line (`[G]Amazing`) are kept; the app strips them for the
//!   screens.
//! - **Quotes and spacing** are tidied: curly quotes stay as they are, tabs
//!   and runs of spaces become one space, and a line's ends are trimmed.
//!
//! Every guess is reported in `warnings`, in plain words, so the person can
//! see what was done and correct it.

use lampstand_core::song::Song;

use super::songselect::{normalise_label, parse_txt, trim_blank, SectionBuilder};

/// What a paste made.
#[derive(Debug, Clone, PartialEq)]
pub struct Pasted {
    pub song: Song,
    /// What was guessed or left out, in words for the person.
    pub warnings: Vec<String>,
}

/// Reads pasted lyrics. `title` is what the person typed in the title box,
/// if anything. An error says why nothing could be made.
pub fn parse(text: &str, title: Option<&str>) -> Result<Pasted, String> {
    let text = text
        .trim_start_matches('\u{feff}')
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let typed = title.map(str::trim).filter(|t| !t.is_empty());
    if text.trim().is_empty() {
        return Err("Paste the words of a song.".into());
    }

    // A SongSelect lyrics page keeps its own layout, footer and all.
    if text.lines().any(|l| {
        l.trim_start()
            .to_ascii_lowercase()
            .starts_with("ccli song #")
    }) {
        if let Ok(mut song) = parse_txt(&text) {
            let mut warnings = vec![
                "Read as a SongSelect lyrics page, with its authors and licence line.".to_string(),
            ];
            if let Some(t) = typed {
                song.title = t.to_string();
            } else if song.title.is_empty() {
                warnings.push("The title is missing.".into());
            }
            return Ok(Pasted { song, warnings });
        }
    }

    let lines: Vec<String> = text.lines().map(tidy).collect();
    let mut blocks: Vec<Vec<String>> = Vec::new();
    for line in lines {
        if line.is_empty() {
            if blocks.last().is_some_and(|b| !b.is_empty()) {
                blocks.push(Vec::new());
            }
        } else {
            if blocks.is_empty() {
                blocks.push(Vec::new());
            }
            blocks.last_mut().unwrap().push(line);
        }
    }
    blocks.retain(|b| !b.is_empty());

    let mut warnings = Vec::new();
    let mut chord_lines = 0usize;

    // The title.
    let mut found_title: Option<String> = None;
    if let Some(first) = blocks.first() {
        if first.len() == 1 && heading_of(&first[0]).is_none() && blocks.len() > 1 {
            let line = first[0].clone();
            match typed {
                Some(t) if t.eq_ignore_ascii_case(&line) => {
                    blocks.remove(0);
                }
                Some(_) => {}
                None => {
                    found_title = Some(line);
                    blocks.remove(0);
                }
            }
        }
    }
    if let Some(t) = &found_title {
        warnings.push(format!(
            "Took “{t}” as the title. Change it if that is wrong."
        ));
    }

    let mut builder = SectionBuilder::default();
    let mut pending: Option<String> = None;
    for block in blocks {
        let mut rest: &[String] = &block;
        let mut heading = pending.take();
        if let Some(h) = heading_of(&rest[0]) {
            heading = Some(h);
            rest = &rest[1..];
        }
        let mut kept: Vec<String> = Vec::new();
        for line in rest {
            if is_chord_line(line) {
                chord_lines += 1;
            } else {
                kept.push(line.clone());
            }
        }
        let kept = trim_blank(kept);
        match (heading, kept.is_empty()) {
            // A heading and nothing under it: written again, it is a repeat;
            // written first, the words follow after a blank line.
            (Some(h), true) => match builder.lines_of(&h) {
                Some(lines) => builder.push(&h, lines),
                None => pending = Some(h),
            },
            (h, false) => builder.push(h.as_deref().unwrap_or(""), kept),
            (None, true) => {}
        }
    }
    if let Some(h) = pending {
        warnings.push(format!("“{h}” has no words under it, so it was left out."));
    }
    if chord_lines > 0 {
        warnings.push(format!(
            "Left out {chord_lines} line{} of chords above the words.",
            if chord_lines == 1 { "" } else { "s" }
        ));
    }

    let title = typed
        .map(str::to_string)
        .or(found_title)
        .unwrap_or_default();
    let song = builder.finish(Song::new(title));
    if song.sections.is_empty() {
        return Err("There are no words to make a song from.".into());
    }
    if song.title.is_empty() {
        warnings.push("The song needs a title.".into());
    }
    let unlabelled = song
        .sections
        .iter()
        .filter(|s| s.label.starts_with('V'))
        .count();
    if song.sections.len() > 1 && unlabelled == song.sections.len() {
        warnings.push("No section headings were found, so every block is a verse. Add Chorus or Bridge headings to say which is which.".into());
    }
    Ok(Pasted { song, warnings })
}

/// A line with its ends trimmed and runs of white space made one space.
fn tidy(line: &str) -> String {
    line.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The heading a line is, in the short form (`V1`, `C`), if it is one:
/// `Verse 1`, `[Chorus]`, `Chorus:`, `(Bridge)`, `V2`, `Pre-Chorus`.
fn heading_of(line: &str) -> Option<String> {
    let inner = line
        .trim()
        .trim_start_matches(['[', '(', '{'])
        .trim_end_matches([']', ')', '}'])
        .trim();
    // `[Chorus x2]`, `Chorus (x2)`: the repeat note is not part of the label.
    let inner = inner
        .split_once(['(', '['])
        .map_or(inner, |(head, _)| head)
        .trim();
    let inner = strip_times(inner);
    normalise_label(inner).or_else(|| normalise_label(&english(inner)?))
}

/// A heading in Spanish, Portuguese, French or German, in English words
/// (`Coro 2` is `Chorus 2`), or `None` when it is not one of these.
fn english(heading: &str) -> Option<String> {
    let heading = heading.trim().trim_end_matches(':').trim();
    let split = heading
        .find(|c: char| c.is_ascii_digit())
        .unwrap_or(heading.len());
    let (word, num) = heading.split_at(split);
    let word: String = word
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'ã' | 'á' | 'à' | 'â' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'î' | 'ï' => 'i',
            'ó' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            c => c,
        })
        .collect();
    let base = match word.as_str() {
        "verso" | "estrofa" | "strophe" | "vers" | "couplet" | "estancia" => "Verse",
        "coro" | "estribillo" | "refrao" | "refrain" | "refrein" | "refran" => "Chorus",
        "puente" | "ponte" | "pont" | "brucke" | "bruecke" => "Bridge",
        "final" | "fin" | "ende" | "conclusao" => "Ending",
        "introduccion" | "introducao" | "einleitung" => "Intro",
        _ => return None,
    };
    Some(format!("{base}{}", num.trim()))
}

/// `Chorus x2` and `Chorus 2x` lose the repeat note.
fn strip_times(s: &str) -> &str {
    let lower = s.to_ascii_lowercase();
    if let Some(at) = lower.rfind(" x") {
        if lower[at + 2..].chars().all(|c| c.is_ascii_digit()) && !lower[at + 2..].is_empty() {
            return s[..at].trim();
        }
    }
    s
}

/// Whether a line is only chords: `G  D/F#  Em7  C`, `Am`, `| C | F |`.
fn is_chord_line(line: &str) -> bool {
    let mut any = false;
    for token in line.split_whitespace() {
        let token = token.trim_matches(|c| matches!(c, '|' | '(' | ')' | '-' | '.' | ':'));
        if token.is_empty() {
            continue;
        }
        if !is_chord(token) {
            return false;
        }
        any = true;
    }
    any
}

fn is_chord(token: &str) -> bool {
    // Slash chords: both sides are chords or bass notes.
    let mut parts = token.split('/');
    let first = parts.next().unwrap_or("");
    if !is_chord_body(first) {
        return false;
    }
    parts.all(is_note)
}

fn is_note(s: &str) -> bool {
    let mut c = s.chars();
    matches!(c.next(), Some('A'..='G')) && matches!(c.as_str(), "" | "#" | "b")
}

fn is_chord_body(s: &str) -> bool {
    let mut c = s.chars();
    if !matches!(c.next(), Some('A'..='G')) {
        return false;
    }
    let rest = c.as_str();
    let rest = rest.strip_prefix(['#', 'b']).unwrap_or(rest);
    // The quality and extensions: m, maj7, sus4, add9, dim, aug, 7, 9, 13, 6, 2, 4, (b5) ...
    let mut tail = rest;
    loop {
        let before = tail;
        for q in [
            "maj", "min", "dim", "aug", "sus", "add", "m", "M", "+", "°", "ø",
        ] {
            if let Some(t) = tail.strip_prefix(q) {
                tail = t;
            }
        }
        let digits = tail.trim_start_matches(|c: char| c.is_ascii_digit());
        tail = digits;
        for q in ["#", "b"] {
            if let Some(t) = tail.strip_prefix(q) {
                if t.starts_with(|c: char| c.is_ascii_digit()) {
                    tail = t;
                }
            }
        }
        if tail.is_empty() {
            return true;
        }
        if tail == before {
            return false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(p: &Pasted) -> Vec<&str> {
        p.song.sections.iter().map(|s| s.label.as_str()).collect()
    }

    #[test]
    fn headed_sections_are_read_and_a_title_line_is_taken() {
        let p = parse(
            "Amazing Grace\n\nVerse 1\nAmazing grace, how sweet the sound\nThat saved a wretch like me\n\nChorus\nMy chains are gone\nI've been set free\n",
            None,
        )
        .unwrap();
        assert_eq!(p.song.title, "Amazing Grace");
        assert_eq!(labels(&p), ["V1", "C"]);
        assert_eq!(p.song.sections[0].lines.len(), 2);
        assert!(p.song.arrangement.is_empty());
        assert!(p
            .warnings
            .iter()
            .any(|w| w.contains("Took “Amazing Grace” as the title")));
    }

    #[test]
    fn a_typed_title_wins_and_the_same_line_pasted_is_not_a_lyric() {
        let p = parse(
            "amazing grace\n\nVerse 1\nline one\nline two\n",
            Some("Amazing Grace"),
        )
        .unwrap();
        assert_eq!(p.song.title, "Amazing Grace");
        assert_eq!(p.song.sections.len(), 1);
        assert!(p.warnings.iter().all(|w| !w.contains("title")));
        // A different typed title leaves a one-line first block alone as words.
        let q = parse("Opening line\n\nVerse 1\nline one\n", Some("My Title")).unwrap();
        assert_eq!(q.song.title, "My Title");
        assert_eq!(q.song.sections.len(), 2);
    }

    #[test]
    fn the_usual_ways_of_writing_a_heading_are_all_headings() {
        for h in [
            "Verse 1",
            "verse 1",
            "[Verse 1]",
            "Verse 1:",
            "(Verse 1)",
            "V1",
            "[V1]",
            "VERSE 1",
        ] {
            assert_eq!(heading_of(h).as_deref(), Some("V1"), "{h}");
        }
        for (h, want) in [
            ("Chorus", "C"),
            ("[Chorus x2]", "C"),
            ("Chorus (x2)", "C"),
            ("Pre-Chorus", "P"),
            ("Bridge 2", "B2"),
            ("Tag", "T"),
            ("Intro", "I"),
            ("Outro", "E"),
        ] {
            assert_eq!(heading_of(h).as_deref(), Some(want), "{h}");
        }
        for not in [
            "Amazing grace, how sweet the sound",
            "Verse of the day is here",
            "My chains are gone",
        ] {
            assert_eq!(heading_of(not), None, "{not}");
        }
    }

    #[test]
    fn a_heading_written_again_with_nothing_under_it_is_a_repeat() {
        let p = parse(
            "Verse 1\nline a\nline b\n\nChorus\nsing it\nsing it again\n\nVerse 2\nline c\nline d\n\nChorus\n",
            Some("Song"),
        )
        .unwrap();
        assert_eq!(labels(&p), ["V1", "C", "V2"]);
        assert_eq!(p.song.arrangement, ["V1", "C", "V2", "C"]);
    }

    #[test]
    fn a_heading_on_its_own_line_takes_the_block_after_the_blank_line() {
        let p = parse(
            "Verse 1\n\nline a\nline b\n\nChorus\n\nsing it\n",
            Some("Song"),
        )
        .unwrap();
        assert_eq!(labels(&p), ["V1", "C"]);
        assert_eq!(p.song.sections[0].lines, ["line a", "line b"]);
    }

    #[test]
    fn blocks_with_no_headings_are_numbered_verses_and_the_result_says_so() {
        let p = parse("one\ntwo\n\nthree\nfour\n\nfive\nsix\n", Some("Song")).unwrap();
        assert_eq!(labels(&p), ["V1", "V2", "V3"]);
        assert!(p
            .warnings
            .iter()
            .any(|w| w.contains("every block is a verse")));
    }

    #[test]
    fn one_block_is_one_verse_without_that_warning() {
        let p = parse("line one\nline two\n", Some("Song")).unwrap();
        assert_eq!(labels(&p), ["V1"]);
        assert!(p.warnings.is_empty(), "{:?}", p.warnings);
    }

    #[test]
    fn lines_of_chords_above_the_words_are_left_out_and_counted() {
        let p = parse(
            "Verse 1\nG        D/F#     Em7\nAmazing grace how sweet\nC    G/B  Am\nThe sound that saved\n",
            Some("Song"),
        )
        .unwrap();
        assert_eq!(
            p.song.sections[0].lines,
            ["Amazing grace how sweet", "The sound that saved"]
        );
        assert!(p
            .warnings
            .iter()
            .any(|w| w == "Left out 2 lines of chords above the words."));
    }

    #[test]
    fn chord_lines_are_told_from_lyrics() {
        for chords in [
            "G D Em C",
            "Am7  Dsus4  G/B",
            "| C | F | G |",
            "F#m7b5  B7",
            "Cmaj7 Fadd9",
            "D/F#",
            "Bb  Eb",
        ] {
            assert!(is_chord_line(chords), "{chords}");
        }
        for words in [
            "A new song",
            "Amazing grace",
            "Be still",
            "Come and see",
            "God is good",
            "Bless the Lord",
            "Gone are the chains",
            "Add one more",
        ] {
            assert!(!is_chord_line(words), "{words}");
        }
    }

    #[test]
    fn chordpro_chords_inside_a_line_are_kept() {
        let p = parse("Verse 1\n[G]Amazing [D]grace\n", Some("Song")).unwrap();
        assert_eq!(p.song.sections[0].lines, ["[G]Amazing [D]grace"]);
    }

    #[test]
    fn white_space_and_line_endings_are_tidied() {
        let p = parse(
            "\u{feff}Verse 1\r\n  Amazing\t grace  \r\n\r\n\r\n\r\nChorus\r\nsing\r\n",
            Some("S"),
        )
        .unwrap();
        assert_eq!(labels(&p), ["V1", "C"]);
        assert_eq!(p.song.sections[0].lines, ["Amazing grace"]);
    }

    #[test]
    fn a_songselect_lyrics_page_keeps_its_authors_and_licence_line() {
        let page = "Example Song\n\nVerse 1\nFirst line\nSecond line\n\nChorus\nSing it out\n\nCCLI Song # 1234567\nA. Writer | B. Writer\n© 2020 Some Publisher\nCCLI License # 1\n";
        let p = parse(page, None).unwrap();
        assert_eq!(p.song.title, "Example Song");
        assert_eq!(p.song.ccli_number, Some(1234567));
        assert_eq!(p.song.author.as_deref(), Some("A. Writer, B. Writer"));
        assert_eq!(labels(&p), ["V1", "C"]);
        assert!(p.warnings[0].starts_with("Read as a SongSelect lyrics page"));
        let typed = parse(page, Some("My Title")).unwrap();
        assert_eq!(typed.song.title, "My Title");
    }

    #[test]
    fn nothing_to_read_says_so() {
        assert_eq!(parse("", None).unwrap_err(), "Paste the words of a song.");
        assert_eq!(
            parse("  \n\n \t\n", None).unwrap_err(),
            "Paste the words of a song."
        );
        // Only chords is no song.
        assert_eq!(
            parse("G D Em C\nAm F\n", Some("S")).unwrap_err(),
            "There are no words to make a song from."
        );
    }

    #[test]
    fn a_missing_title_is_called_out_not_invented() {
        let p = parse("Verse 1\nline a\nline b\n\nChorus\nsing\n", None).unwrap();
        assert_eq!(p.song.title, "");
        assert!(p.warnings.iter().any(|w| w == "The song needs a title."));
    }

    #[test]
    fn non_english_and_curly_quotes_pass_through() {
        let p = parse(
            "Verso 1\n¡Aleluya! Cantad al Señor\n“Gracia” y paz\n",
            Some("Gracia"),
        )
        .unwrap();
        assert_eq!(
            p.song.sections[0].lines,
            ["¡Aleluya! Cantad al Señor", "“Gracia” y paz"]
        );
    }

    #[test]
    fn a_very_long_paste_is_read_without_trouble() {
        let mut text = String::new();
        for n in 0..2000 {
            text.push_str(&format!("Verse {}\nline {n} a\nline {n} b\n\n", n % 90 + 1));
        }
        let p = parse(&text, Some("Long")).unwrap();
        assert!(p.song.sections.len() > 90);
    }
}
