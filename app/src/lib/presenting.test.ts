import { test } from "node:test";
import assert from "node:assert/strict";
import type { PresenterView, TeamStatus } from "./api";
import { howLong, isPlanning, presentChoice, replacedNotice, requestPrompt, withRole } from "./presenting.ts";

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
  assert.deepEqual(withRole(pill, lone), pill, "no one paired: unchanged");
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

test("the button depends on how this device can become the presenter", () => {
  assert.deepEqual(presentChoice(view({ role: "Presenting", standing: "Presenting" })), { kind: "go" });
  assert.deepEqual(presentChoice(null), { kind: "go" });
  assert.deepEqual(presentChoice(view()), { kind: "ask", label: "Ask to present" });
  assert.deepEqual(presentChoice(view({ standing: "Open" })), { kind: "claim", label: "Present from here" });
  assert.deepEqual(presentChoice(view({ standing: "Open" }), "Make this device the presenter"), {
    kind: "claim",
    label: "Make this device the presenter",
  });
});

test("an ask is waiting, then allowed or declined", () => {
  assert.deepEqual(presentChoice(view({ asking: { declined: false } })), { kind: "waiting", label: "Waiting for Mary's laptop to allow…" });
  assert.deepEqual(presentChoice(view({ asking: { declined: true } })), { kind: "declined", label: "Mary's laptop said not now", again: "Ask again" });
});

test("taking over says how long the presenter has been offline", () => {
  const c = presentChoice(view({ standing: "TakeOver", offline_secs: 300 }));
  assert.equal(c.kind, "takeover");
  assert.equal(c.kind === "takeover" && c.confirm, "Mary's laptop has been offline for 5 minutes. Present from here?");
  const w = presentChoice(view({ standing: "Wait", offline_secs: 30, wait_secs: 90 }));
  assert.equal(w.kind === "wait" && w.hint, "Mary's laptop went offline 30 seconds ago. You can take over in 2 minutes.");
});

test("the presenter's prompt warns when something is on air", () => {
  assert.equal(requestPrompt("Mary's laptop", false), "Mary's laptop wants to present.");
  assert.match(requestPrompt("Mary's laptop", true), /on air here/);
  assert.match(replacedNotice("Mary's laptop"), /now planning/);
});
