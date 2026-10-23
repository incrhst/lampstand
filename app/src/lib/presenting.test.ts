import { test } from "node:test";
import assert from "node:assert/strict";
import type { PresenterView, TeamStatus } from "./api";
import { howLong, isPlanning, presentChoice, replacedNotice, requestPrompt, syncLabel } from "./presenting.ts";

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
  paired: [{ device: "aa", fingerprint: "", name: "Mary's laptop", address: null, connected, presenting: presenter.role === "Planning" }],
  nearby: [],
  presenter,
});

test("durations read plainly", () => {
  assert.equal(howLong(1), "1 second");
  assert.equal(howLong(45), "45 seconds");
  assert.equal(howLong(60), "1 minute");
  assert.equal(howLong(300), "5 minutes");
  assert.equal(howLong(7200), "2 hours");
});

test("a device with no one paired says so and never plans", () => {
  const lone = team(view({ role: "Presenting", standing: "Presenting" }));
  lone.paired = [];
  assert.equal(syncLabel(lone), "This device only");
  assert.equal(syncLabel(null), "This device only");
  assert.equal(isPlanning(lone), false);
});

test("the status bar names the role and who presents", () => {
  assert.equal(syncLabel(team(view({ role: "Presenting", standing: "Presenting" }))), "Presenting · 2 devices synced");
  assert.equal(syncLabel(team(view({ role: "Presenting", standing: "Presenting" }), false)), "Presenting · other devices offline");
  assert.equal(syncLabel(team(view())), "Planning · Mary's laptop is presenting");
  assert.equal(syncLabel(team(view({ standing: "Open" }))), "Planning · nobody is presenting");
  assert.equal(syncLabel(team(view({ standing: "Wait" }), false)), "Planning · Mary's laptop is offline");
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
