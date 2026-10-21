//! Telling the project about a problem or an idea, and asking for help.
//!
//! "Report a problem" and "Suggest an idea" open the project's GitHub issue
//! form in the browser, and "Ask for help" opens a new question in its
//! Discussions, each with the version and the kind of computer already
//! filled in, so the person only has to say what they mean. Nothing is sent from here: they
//! read the form and submit it themselves, and there is nothing in it but
//! what they can see. The forms are in `.github/ISSUE_TEMPLATE`.

use lampstand_api::AboutRequest;

/// Where problems and ideas are reported.
pub const ISSUES_URL: &str = "https://github.com/incrhst/lampstand/issues";

/// Where questions are asked and answered.
pub const DISCUSSIONS_URL: &str = "https://github.com/incrhst/lampstand/discussions";

/// The page `request` opens: the issue form for it, with the version and
/// computer filled in (the form's `version` field).
pub fn page(request: &AboutRequest, version: &str, os: &str, arch: &str) -> String {
    let computer = format!("Lampstand {version} on {os} ({arch})");
    match request {
        AboutRequest::ReportProblem => issue_form("bug_report.yml", &computer),
        AboutRequest::SuggestIdea => issue_form("idea.yml", &computer),
        // Discussions has no form fields, so the version leads the question.
        AboutRequest::AskForHelp => format!(
            "{DISCUSSIONS_URL}/new?category=q-a&body={}",
            encode(&format!("{computer}\n\n"))
        ),
    }
}

fn issue_form(template: &str, computer: &str) -> String {
    format!(
        "{ISSUES_URL}/new?template={template}&version={}",
        encode(computer)
    )
}

/// A friendly name for the operating system this build runs on.
pub fn os_name() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macOS",
        "windows" => "Windows",
        "linux" => "Linux",
        other => other,
    }
}

/// Percent-encodes `text` for a URL query value.
fn encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_problem_opens_the_bug_form_with_the_version_and_computer_filled_in() {
        let url = page(&AboutRequest::ReportProblem, "0.1.0", "macOS", "aarch64");
        assert_eq!(
            url,
            "https://github.com/incrhst/lampstand/issues/new?template=bug_report.yml&version=Lampstand%200.1.0%20on%20macOS%20%28aarch64%29"
        );
    }

    #[test]
    fn an_idea_opens_the_idea_form() {
        let url = page(&AboutRequest::SuggestIdea, "0.2.0", "Windows", "x86_64");
        assert!(
            url.starts_with("https://github.com/incrhst/lampstand/issues/new?template=idea.yml&")
        );
        assert!(url.ends_with("Lampstand%200.2.0%20on%20Windows%20%28x86_64%29"));
    }

    #[test]
    fn a_question_opens_a_new_discussion_in_questions_and_answers_with_the_version_at_the_top() {
        let url = page(&AboutRequest::AskForHelp, "0.1.0", "macOS", "aarch64");
        assert_eq!(
            url,
            "https://github.com/incrhst/lampstand/discussions/new?category=q-a&body=Lampstand%200.1.0%20on%20macOS%20%28aarch64%29%0A%0A"
        );
    }

    #[test]
    fn only_the_version_and_computer_are_in_the_address() {
        // Nothing about the church, its songs or its settings can reach the
        // address: it is built from three short strings.
        let url = page(&AboutRequest::ReportProblem, "0.1.0", "Linux", "x86_64");
        assert!(!url.contains(' '), "{url}");
        assert_eq!(
            url.matches('&').count(),
            1,
            "template and version only: {url}"
        );
    }

    #[test]
    fn awkward_characters_are_encoded() {
        assert_eq!(encode("a b&c=d#e/é"), "a%20b%26c%3Dd%23e%2F%C3%A9");
    }
}
