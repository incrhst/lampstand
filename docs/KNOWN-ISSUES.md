# Known issues

## Takeover test is skipped on Linux (flaky on Ubuntu CI)

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

Update (PR #148): the cut in this test and in the app tests is now atomic
(`SyncNode::set_offline`: while offline no connection can register, so none
survives a cut). The test still could not be made to fail locally, with or
without that change, under 16 parallel copies, 6 CPU burners and two worker
threads (64 runs each), so whether this fixes the Ubuntu failure is unknown;
it stays ignored on Linux until a CI run says so. A different cause of
flaky takeover tests was found and fixed in the app tests: a takeover time
shorter than the five-second heartbeat lets the claimant count its absence
from an earlier last frame than the holder counts from, so the holder can
rightly refuse a claim made too soon after the cut. Those tests now wait
until the claimant really was out of reach for the takeover time. This test
sets a two-second takeover too and may be exposed to it.
