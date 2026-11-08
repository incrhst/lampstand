import { test } from "node:test";
import assert from "node:assert/strict";
import type { SyncSummary } from "./api/index.ts";
import { ago, syncFailed, syncPill } from "./sync.ts";

const base: SyncSummary = { state: "Synced", paired: 2, connected: 2, reason: null, last_in_ms: null, last_out_ms: null };

test("the pill says this device only before anything is known or paired", () => {
  assert.equal(syncPill(null).text, "This device only");
  assert.equal(syncPill({ ...base, state: "DeviceOnly", paired: 0, connected: 0 }).text, "This device only");
});

test("the pill counts the connected devices", () => {
  assert.equal(syncPill(base).text, "Synced with 2 devices");
  assert.equal(syncPill({ ...base, paired: 1, connected: 1 }).text, "Synced with 1 device");
});

test("the pill says syncing while changes move", () => {
  assert.equal(syncPill({ ...base, state: "Syncing" }).text, "Syncing…");
});

test("the pill counts the devices that are not reachable, in amber", () => {
  const pill = syncPill({ ...base, state: "Offline", connected: 1 });
  assert.equal(pill.tone, "warn");
  assert.equal(pill.text, "Offline: 1 device not reachable");
  assert.equal(syncPill({ ...base, state: "Offline", connected: 0 }).text, "Offline: 2 devices not reachable");
});

test("the pill gives the reason when sync failed", () => {
  const pill = syncPill(syncFailed("Sync is off: the port is taken"));
  assert.equal(pill.tone, "warn");
  assert.equal(pill.title, "Sync is off: the port is taken");
});

test("ago reads in plain units", () => {
  assert.equal(ago(null, 1000), "none yet");
  assert.equal(ago(0, 4000), "just now");
  assert.equal(ago(0, 30_000), "30 seconds ago");
  assert.equal(ago(0, 180_000), "3 minutes ago");
  assert.equal(ago(0, 7_200_000), "2 hours ago");
});
