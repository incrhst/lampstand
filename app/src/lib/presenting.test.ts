import { test } from "node:test";
import assert from "node:assert/strict";
import type { PresenterView, TeamStatus } from "./api";
import { isPlanning } from "./presenting.ts";

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
  scene_cue_waiting: false,
  sync: { state: "Synced", paired: 1, connected: connected ? 1 : 0, reason: null, last_in_ms: null, last_out_ms: null },
});

test("a device that does not run the screens only plans", () => {
  assert.equal(isPlanning(team(view())), true);
  assert.equal(isPlanning(team(view({ role: "Presenting", standing: "Presenting" }))), false);
  assert.equal(isPlanning(null), false);
});
