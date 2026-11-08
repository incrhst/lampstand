import { test } from "node:test";
import assert from "node:assert/strict";
import type { DeviceView, DriveView, PresenterView, TeamStatus } from "./api";
import { driverOffline, drivenBy, drivingFrom, isDriving } from "./driving.ts";

const quiet: DriveView = {
  driving: null,
  active: null,
  requests: [],
  asking: null,
  can_drive: false,
  always: [],
  taken_back_by: null,
};
const presenter = (over: Partial<PresenterView> = {}, drive: Partial<DriveView> = {}): PresenterView => ({
  role: "Planning",
  standing: "Ask",
  holder: "aa",
  holder_name: "Sanctuary PC",
  epoch: 1,
  offline_secs: null,
  wait_secs: null,
  takeover_secs: 120,
  requests: [],
  asking: null,
  replaced_by: null,
  drive: { ...quiet, ...drive },
  ...over,
});
const device = (key: string, name: string, presenting: boolean): DeviceView => ({
  device: key,
  fingerprint: "",
  name,
  address: null,
  connected: true,
  problem: null,
  presenting,
});
const team = (p: PresenterView, paired: DeviceView[]): TeamStatus => ({
  name: "Mary's laptop",
  fingerprint: "",
  addresses: [],
  pairing: null,
  paired,
  nearby: [],
  presenter: p,
  roles: [],
  scene_cue_waiting: false,
  sync: { state: "Synced", paired: paired.length, connected: paired.length, reason: null, last_in_ms: null, last_out_ms: null },
});

const driverTeam = (online = true) =>
  team(presenter({}, { driving: { presenter: "aa", name: "Sanctuary PC", online } }), [device("aa", "Sanctuary PC", true)]);
const presenterTeam = () =>
  team(presenter({ role: "Presenting", standing: "Presenting", holder_name: "Sanctuary PC" }, { active: { device: "bb", name: "Mary's laptop" } }), [
    device("bb", "Mary's laptop", false),
  ]);

test("a device that runs the service for another's screens, and the one it runs it for", () => {
  assert.equal(isDriving(driverTeam()), true);
  assert.equal(drivenBy(presenterTeam()), "Mary's laptop");
  assert.equal(isDriving(presenterTeam()), false);
  const idle = team(presenter(), [device("aa", "Sanctuary PC", true)]);
  assert.equal(drivenBy(idle), null);
  assert.equal(isDriving(idle), false);
  assert.equal(drivingFrom(null), null);
});

test("the service controls wait while the device that runs the screens is offline", () => {
  assert.equal(driverOffline(driverTeam(false)), true);
  assert.equal(driverOffline(driverTeam(true)), false);
  assert.equal(driverOffline(idleTeam()), false);
});

function idleTeam() {
  return team(presenter(), [device("aa", "Sanctuary PC", true)]);
}
