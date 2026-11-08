//! Text slides: how the body of an `ItemKind::Text` becomes slides.
//!
//! The planner types plain text. A blank line, or a line of three or more
//! hyphens on its own, starts a new slide; line breaks inside a slide are
//! kept. Fitting the text to the screen is the compositor's job, as for
//! lyrics and scripture.

use crate::program::SlideText;

/// The slides of a text item. An empty body gives none. `attribution`, when
/// not blank, is the footer of every slide.
pub fn text_slides(body: &str, attribution: Option<&str>) -> Vec<SlideText> {
    let footer = attribution
        .map(str::trim)
        .filter(|a| !a.is_empty())
        .map(str::to_string);
    let mut groups: Vec<Vec<String>> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    for line in body.lines() {
        let line = line.trim();
        let separator = line.len() >= 3 && line.bytes().all(|b| b == b'-');
        if line.is_empty() || separator {
            if !current.is_empty() {
                groups.push(std::mem::take(&mut current));
            }
        } else {
            current.push(line.to_string());
        }
    }
    if !current.is_empty() {
        groups.push(current);
    }
    let total = groups.len();
    groups
        .into_iter()
        .enumerate()
        .map(|(i, lines)| SlideText {
            label: Some(if total > 1 {
                format!("{}/{total}", i + 1)
            } else {
                "Text".to_string()
            }),
            lines,
            footer: footer.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blank_line_starts_a_new_slide() {
        let slides = text_slides("Welcome\nto church\n\n\n  \nPlease sit", None);
        assert_eq!(slides.len(), 2);
        assert_eq!(slides[0].lines, ["Welcome", "to church"]);
        assert_eq!(slides[1].lines, ["Please sit"]);
        assert_eq!(slides[1].label.as_deref(), Some("2/2"));
    }

    #[test]
    fn a_line_of_hyphens_is_a_separator_and_is_never_shown() {
        let slides = text_slides("One\n---\nTwo\n-----\n\nThree\n- a list item", None);
        let lines: Vec<_> = slides.iter().map(|s| s.lines.clone()).collect();
        assert_eq!(
            lines,
            [vec!["One"], vec!["Two"], vec!["Three", "- a list item"]]
        );
    }

    #[test]
    fn attribution_is_the_footer_of_every_slide() {
        let slides = text_slides("A\n\nB", Some("  C. S. Lewis "));
        assert!(slides
            .iter()
            .all(|s| s.footer.as_deref() == Some("C. S. Lewis")));
    }

    #[test]
    fn blank_attribution_and_empty_body() {
        assert!(text_slides(" \n\n---\n", Some("x")).is_empty());
        let one = text_slides("Hi", Some("  "));
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].footer, None);
        assert_eq!(one[0].label.as_deref(), Some("Text"));
    }
}
