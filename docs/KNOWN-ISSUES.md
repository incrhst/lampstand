# Known issues

## Takeover test is skipped on Linux (flaky on Ubuntu CI)

**Status:** Linux support is paused. CI no longer runs the main checks on
Linux (only macOS and Windows), and no Linux installers are built. The skip
below stays in the code for when Linux comes back; the issue stays open.

Tracking issue: https://github.com/incrhst/lampstand/issues/144

`takeover_waits_for_the_timeout_and_the_old_presenter_yields_on_return`
(`crates/lampstand-sync/tests/net.rs`) fails intermittently on Ubuntu CI.

- **Symptom:** the office PC's standing stays `Ask` after the network is
  cut, and the test gives up after 10 s with "timed out waiting for takeover
  to open" (log: `connected 0 standing Wait`, then `connected 1 standing Ask`).
- **How often:** twice in a row on PR #143, and once on main at 8ad1a64. Never
  seen on macOS or Windows CI.
- **What was tried:** several test-side fixes (cut both peers, give the cut
  time to be noticed, log what the office saw). Not reproduced locally on
  macOS in about 230 runs, including under heavy CPU load.
- **Leading hypothesis (unproven):** the test's "cut" is not atomic, so a
  connection in flight at the moment of the cut can register a session
  afterwards and stay up. See the issue for details.

The test is `ignore`d on Linux only (`cargo test -- --ignored` runs it). It
still runs on macOS and Windows CI and locally. The takeover path is also
exercised there and by the two-device tests, but it is **not** exercised on
Linux until this is fixed. That means a real Linux-only takeover race, if one
exists, could be hidden by this skip.
