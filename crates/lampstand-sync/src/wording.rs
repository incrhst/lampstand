//! What a volunteer reads when a role cannot be taken or handed over.
//!
//! Wording is per role, so it never calls the holder "the presenting
//! device": the roles spec (docs/roles.md) names the jobs "the screens",
//! "the service", "the stream" and "the sound". Where the spec gives the
//! words they are used as written ("No screens are set up on this device");
//! the rest follow its pattern and are listed in the pull request for
//! review. The web app has the same nouns in `app/src/lib/roles.ts`.

use crate::roles::{Denied, NotAllowed, Role};

/// What the role runs, as the roster says it: "Runs the screens".
/// Why the Operator cannot be taken or handed over.
pub const NEEDS_SCREENS: &str = "The service can't be run until someone runs the screens.";

pub fn job(role: Role) -> &'static str {
    match role {
        Role::Projection => "the screens",
        Role::Operator => "the service",
        Role::Stream => "the stream",
        Role::Audio => "the sound",
    }
}

/// "30 seconds", "1 minute", "5 minutes", "2 hours": how long, for people
/// (the same as the web app's `howLong`).
pub fn how_long(d: std::time::Duration) -> String {
    let s = d.as_secs();
    if s < 60 {
        return if s == 1 {
            "1 second".into()
        } else {
            format!("{s} seconds")
        };
    }
    let m = (s + 30) / 60;
    if m < 60 {
        return if m == 1 {
            "1 minute".into()
        } else {
            format!("{m} minutes")
        };
    }
    let h = (m + 30) / 60;
    if h == 1 {
        "1 hour".into()
    } else {
        format!("{h} hours")
    }
}

fn who(role: Role, holder: &str) -> String {
    if holder.trim().is_empty() {
        format!("The device that runs {}", job(role))
    } else {
        holder.to_string()
    }
}

/// Why a claim or an ask was refused. `holder` is the holder's name, or
/// empty when it is not known.
pub fn denied(role: Role, why: &Denied, holder: &str) -> String {
    let job = job(role);
    match why {
        Denied::MustAsk => format!("{} is connected: ask it to allow this.", who(role, holder)),
        Denied::TooSoon { remaining, .. } => format!(
            "{} went offline only recently. You can take over running {job} in {}.",
            who(role, holder),
            how_long((*remaining).max(std::time::Duration::from_secs(1)))
        ),
        Denied::Already => format!("This device already runs {job}."),
        Denied::NobodyToAsk => {
            format!(
                "{} is not connected, so it cannot be asked.",
                who(role, holder)
            )
        }
        Denied::NotNeeded => "Nobody needs to allow this. Take it from this device.".to_string(),
        // The spec's own words.
        Denied::NoScreens => "No screens are set up on this device".to_string(),
        Denied::NeedsProjection => NEEDS_SCREENS.to_string(),
        Denied::NotPermitted => format!("This device may not run {job}."),
    }
}

/// Why the holder could not hand the role over.
pub fn not_allowed(role: Role, why: &NotAllowed) -> String {
    let job = job(role);
    match why {
        NotAllowed::NotHolding => format!("This device does not run {job}."),
        NotAllowed::NotAsking => "That device is no longer asking.".to_string(),
        NotAllowed::NeedsProjection => NEEDS_SCREENS.to_string(),
        NotAllowed::NotPermitted => format!("That device may not run {job}."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn no_text_calls_the_holder_the_presenting_device() {
        let denials = [
            Denied::MustAsk,
            Denied::TooSoon {
                offline: Duration::from_secs(5),
                remaining: Duration::from_secs(30),
            },
            Denied::Already,
            Denied::NobodyToAsk,
            Denied::NotNeeded,
            Denied::NoScreens,
            Denied::NeedsProjection,
            Denied::NotPermitted,
        ];
        let refusals = [
            NotAllowed::NotHolding,
            NotAllowed::NotAsking,
            NotAllowed::NeedsProjection,
            NotAllowed::NotPermitted,
        ];
        for role in Role::ALL {
            for d in &denials {
                for holder in ["", "Mary's laptop"] {
                    let text = denied(role, d, holder);
                    assert!(!text.contains("present"), "{role:?} {d:?}: {text}");
                }
            }
            for n in &refusals {
                assert!(!not_allowed(role, n).contains("present"));
            }
        }
    }

    #[test]
    fn durations_read_plainly() {
        let secs = Duration::from_secs;
        assert_eq!(how_long(secs(1)), "1 second");
        assert_eq!(how_long(secs(30)), "30 seconds");
        assert_eq!(how_long(secs(60)), "1 minute");
        assert_eq!(how_long(secs(120)), "2 minutes");
        assert_eq!(how_long(secs(7200)), "2 hours");
        assert_eq!(
            denied(
                Role::Stream,
                &Denied::TooSoon {
                    offline: secs(3),
                    remaining: secs(30)
                },
                "Sam"
            ),
            "Sam went offline only recently. You can take over running the stream in 30 seconds."
        );
    }

    #[test]
    fn the_spec_words_are_used_as_written() {
        assert_eq!(
            denied(Role::Projection, &Denied::NoScreens, ""),
            "No screens are set up on this device"
        );
        assert_eq!(
            denied(Role::Stream, &Denied::Already, ""),
            "This device already runs the stream."
        );
        assert_eq!(
            denied(Role::Audio, &Denied::MustAsk, "Mary's laptop"),
            "Mary's laptop is connected: ask it to allow this."
        );
    }
}
