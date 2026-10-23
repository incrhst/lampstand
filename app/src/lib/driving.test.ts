import { test } from "node:test";
import assert from "node:assert/strict";
import type { DeviceView, DriveView, PresenterView, TeamStatus } from "./api";
import {
  driveBarText,
  driveChoice,
  driverOffline,
  drivenBy,
  isDriving,
  ownRole,
  ownRoleLine,
  roleLine,
} from "./driving.ts";
import { withRole } from "./presenting.ts";

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
  sync: { state: "Synced", paired: paired.length, connected: paired.length, reason: null, last_in_ms: null, last_out_ms: null },
});
const pill = { tone: "ready" as const, text: "Synced with 1 device", title: "Shared" };

const driverTeam = (online = true) =>
  team(presenter({}, { driving: { presenter: "aa", name: "Sanctuary PC", online } }), [device("aa", "Sanctuary PC", true)]);
const presenterTeam = () =>
  team(presenter({ role: "Presenting", standing: "Presenting", holder_name: "Sanctuary PC" }, { active: { device: "bb", name: "Mary's laptop" } }), [
    device("bb", "Mary's laptop", false),
  ]);

test("a driver names the presenter and a presenter names its driver", () => {
  assert.equal(withRole(pill, driverTeam()).text, "Driving Sanctuary PC · Synced with 1 device");
  assert.equal(withRole(pill, presenterTeam()).text, "Presenting · driven from Mary's laptop · Synced with 1 device");
  // A planner that drives nothing still says who presents.
  assert.equal(withRole(pill, team(presenter(), [device("aa", "Sanctuary PC", true)])).text, "Planning · Sanctuary PC is presenting");
});

test("a driver whose presenter is offline says the controls are paused", () => {
  const t = driverTeam(false);
  assert.equal(driverOffline(t), true);
  const shown = withRole(pill, t);
  assert.equal(shown.tone, "warn");
  assert.match(shown.text, /Sanctuary PC · offline, controls paused/);
  assert.match(driveBarText({ presenter: "aa", name: "Sanctuary PC", online: false }), /not connected/);
  assert.equal(driveBarText({ presenter: "aa", name: "Sanctuary PC", online: true }), "Driving Sanctuary PC");
});

test("each device shows Presenting, Driving or Planning", () => {
  const p = presenterTeam();
  assert.equal(ownRole(p), "Presenting");
  assert.equal(ownRoleLine(p), "Presenting · driven from Mary's laptop");
  assert.equal(roleLine(p, p.paired[0]), "Driving Sanctuary PC");
  const d = driverTeam();
  assert.equal(ownRole(d), "Driving");
  assert.equal(ownRoleLine(d), "Driving Sanctuary PC");
  assert.equal(roleLine(d, d.paired[0]), "Presenting");
  const idle = team(presenter(), [device("aa", "Sanctuary PC", true)]);
  assert.equal(ownRole(idle), "Planning");
  assert.equal(drivenBy(idle), null);
  assert.equal(isDriving(idle), false);
  assert.equal(ownRole(null), "Presenting");
});

test("the drive button follows the state of the ask", () => {
  assert.deepEqual(driveChoice(null, "Sanctuary PC"), { kind: "none" });
  assert.deepEqual(driveChoice(quiet, "Sanctuary PC"), { kind: "none" });
  assert.equal(driveChoice({ ...quiet, can_drive: true }, "Sanctuary PC").kind, "ask");
  assert.match((driveChoice({ ...quiet, can_drive: true, asking: "Waiting" }, "Sanctuary PC") as { label: string }).label, /Waiting for Sanctuary PC/);
  assert.equal(driveChoice({ ...quiet, asking: "Declined" }, "Sanctuary PC").kind, "declined");
  const busy = driveChoice({ ...quiet, asking: { Busy: { by: "Sam's tablet" } } }, "Sanctuary PC");
  assert.equal(busy.kind, "busy");
  assert.match((busy as { label: string }).label, /Sam's tablet is driving/);
  assert.equal(driveChoice({ ...quiet, driving: { presenter: "aa", name: "Sanctuary PC", online: true } }, "Sanctuary PC").kind, "driving");
});
