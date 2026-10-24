import { test } from "node:test";
import assert from "node:assert/strict";
import type { PresenterView, TeamStatus } from "./api";
import { howLong, isPlanning, withRole } from "./presenting.ts";

const view = (over: Partial<PresenterView> = {}): PresenterView => ({
  role: "Planning",
  standing: "Ask",
  holder: "aa",
  holder_name: "Mary's laptop",
  epoch: 1,
  offline_secs: null,
  wait_secs: null,
  takeover_secs: 120,
  requests: [],
  asking: null,
  replaced_by: null,
  drive: {
    driving: null,
    active: null,
    requests: [],
    asking: null,
    can_drive: false,
    always: [],
    taken_back_by: null,
  },
  ...over,
});
const team = (presenter: PresenterView, connected = true): TeamStatus => ({
  name: "Office PC",
  fingerprint: "",
  addresses: [],
  pairing: null,
  paired: [{ device: "aa", fingerprint: "", name: "Mary's laptop", address: null, connected, problem: null, presenting: presenter.role === "Planning" }],
  nearby: [],
  presenter,
  roles: [],
  sync: { state: "Synced", paired: 1, connected: connected ? 1 : 0, reason: null, last_in_ms: null, last_out_ms: null },
});

test("durations read plainly", () => {
  assert.equal(howLong(1), "1 second");
  assert.equal(howLong(45), "45 seconds");
  assert.equal(howLong(60), "1 minute");
  assert.equal(howLong(300), "5 minutes");
  assert.equal(howLong(7200), "2 hours");
});

const pill = { tone: "ready" as const, text: "Synced with 1 device", title: "Shared" };

test("the status bar names the role and who presents", () => {
  const lone = team(view({ role: "Presenting", standing: "Presenting" }));
  lone.paired = [];
  const alone = { tone: "neutral" as const, text: "This device only", title: "Nothing is paired" };
  const shown = withRole(alone, lone);
  assert.equal(shown.text, "Presenting · This device only");
  assert.match(shown.title, /has to ask first/);
  assert.deepEqual(withRole(pill, null), pill);
  assert.equal(isPlanning(lone), false);
  assert.equal(withRole(pill, team(view({ role: "Presenting", standing: "Presenting" }))).text, "Presenting · Synced with 1 device");
  assert.equal(withRole(pill, team(view())).text, "Planning · Mary's laptop is presenting");
  assert.equal(withRole(pill, team(view({ standing: "Open" }))).text, "Planning · nobody is presenting");
  assert.equal(withRole(pill, team(view({ standing: "Wait" }), false)).text, "Planning · Mary's laptop is offline");
  const warn = { tone: "warn" as const, text: "Offline: 1 device not reachable", title: "x" };
  assert.equal(withRole(warn, team(view())).text, "Planning · Offline: 1 device not reachable");
  assert.equal(isPlanning(team(view())), true);
});

