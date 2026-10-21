// node --test --experimental-strip-types (npm test)
import { test } from "node:test";
import assert from "node:assert/strict";
import { availableVersion, canInstall, checkDue, CHECK_EVERY_MS, updateSummary } from "./updates.ts";
import type { UpdateState, UpdateStatus } from "./api";

const status = (state: UpdateState, blocked: string | null = null): UpdateStatus => ({ current: "0.1.0", state, blocked });

test("the status line says where updating stands", () => {
  assert.deepEqual(updateSummary(null), { tone: "neutral", text: "Not checked yet" });
  assert.equal(updateSummary(status("UpToDate")).text, "Up to date");
  assert.equal(updateSummary(status("Installing")).text, "Installing…");
  const found = updateSummary(status({ Available: { version: "0.2.0", notes: null } }));
  assert.deepEqual(found, { tone: "gold", text: "Version 0.2.0 is available" });
  const failed = updateSummary(status({ Failed: { message: "no network" } }));
  assert.deepEqual(failed, { tone: "warn", text: "Could not check: no network" });
});

test("only a waiting release that nothing blocks can be installed", () => {
  const waiting: UpdateState = { Available: { version: "0.2.0", notes: "Fixes" } };
  assert.equal(availableVersion(waiting), "0.2.0");
  assert.equal(canInstall(status(waiting)), true);
  assert.equal(canInstall(status(waiting, "A service is on air. Update when it has finished.")), false);
  assert.equal(canInstall(status("UpToDate")), false);
  assert.equal(canInstall(null), false);
  assert.equal(availableVersion("UpToDate"), null);
});

test("it looks again when the last check is old", () => {
  assert.equal(checkDue(null, 1000), true);
  assert.equal(checkDue(1000, 1000 + CHECK_EVERY_MS - 1), false);
  assert.equal(checkDue(1000, 1000 + CHECK_EVERY_MS), true);
});
