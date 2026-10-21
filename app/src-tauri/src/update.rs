//! Updating Lampstand in place (T59).
//!
//! A newer release is found by asking the release page for `latest.json` (the
//! updater plugin; the URL and the key that signs updates are in
//! `tauri.conf.json`). It is installed only at a quiet moment: nothing on air,
//! and no service starting within the hour. This module is that rule; the
//! shell does the asking and installing.

use lampstand_core::id::Timestamp;

/// How long before a service starts that an update is refused.
pub const QUIET_BEFORE_SERVICE_MS: i64 = 60 * 60 * 1000;

/// Why an update must not be installed now, in words for the operator, or
/// `None` when it may be. `idle` is the live engine's: nothing live and
/// nothing on the program. `next_service` is the soonest service start
/// still to come.
pub fn blocker(idle: bool, next_service: Option<Timestamp>, now: Timestamp) -> Option<String> {
    if !idle {
        return Some("A service is on air. Update when it has finished.".into());
    }
    let start = next_service?;
    let wait = start.0 - now.0;
    if wait > 0 && wait <= QUIET_BEFORE_SERVICE_MS {
        let minutes = (wait + 59_999) / 60_000;
        return Some(format!(
            "A service starts in {minutes} {}. Update after it.",
            if minutes == 1 { "minute" } else { "minutes" }
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: Timestamp = Timestamp(1_000_000_000_000);
    const MINUTE: i64 = 60_000;

    #[test]
    fn nothing_on_air_and_no_service_soon_may_update() {
        assert_eq!(blocker(true, None, NOW), None);
        // A service tomorrow is not soon.
        assert_eq!(
            blocker(true, Some(Timestamp(NOW.0 + 24 * 60 * MINUTE)), NOW),
            None
        );
        // Just over an hour away is fine, an hour exactly is not.
        assert_eq!(
            blocker(true, Some(Timestamp(NOW.0 + 61 * MINUTE)), NOW),
            None
        );
    }

    #[test]
    fn something_on_air_blocks_an_update() {
        let why = blocker(false, None, NOW).unwrap();
        assert!(why.contains("on air"), "{why}");
    }

    #[test]
    fn a_service_starting_within_the_hour_blocks_an_update_and_says_how_soon() {
        let why = blocker(true, Some(Timestamp(NOW.0 + 30 * MINUTE)), NOW).unwrap();
        assert_eq!(why, "A service starts in 30 minutes. Update after it.");
        let edge = blocker(true, Some(Timestamp(NOW.0 + 60 * MINUTE)), NOW).unwrap();
        assert!(edge.contains("60 minutes"), "{edge}");
        // Part of a minute counts as the whole minute, never "0 minutes".
        let soon = blocker(true, Some(Timestamp(NOW.0 + 20_000)), NOW).unwrap();
        assert_eq!(soon, "A service starts in 1 minute. Update after it.");
    }

    #[test]
    fn a_service_that_has_already_started_does_not_block_by_time() {
        // It is on air if it is running, and `idle` says so; its start time
        // alone, in the past, is no reason to wait.
        assert_eq!(
            blocker(true, Some(Timestamp(NOW.0 - 5 * MINUTE)), NOW),
            None
        );
    }
}
