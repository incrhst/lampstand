// node --test --experimental-strip-types (npm test)
import { test } from "node:test";
import assert from "node:assert/strict";
import { steppedDown, takesMainPlace, windowToDemote } from "./primary.ts";
import type { OutputConfig, OutputTarget } from "./api";

const win: OutputTarget = { Window: { width: 960, height: 540 } };
const display: OutputTarget = { Display: { name: "S22F350", width: 1920, height: 1080, x: 0, y: 0 } };

const output = (name: string, target: OutputTarget, primary: boolean): OutputConfig => ({
  id: name,
  name,
  role: { Audience: { primary } },
  target,
  safe_area: { top: 0, right: 0, bottom: 0, left: 0 },
  background: "Bleed",
  theme: null,
  idle: null,
  enabled: true,
});

test("the first audience output is the main screen", () => {
  assert.equal(takesMainPlace([], win), true);
  assert.equal(takesMainPlace([], display), true);
});

test("a display takes the main place from a primary window, which steps down", () => {
  const rehearsal = output("Rehearsal", win, true);
  assert.equal(windowToDemote([rehearsal], display), rehearsal);
  assert.equal(takesMainPlace([rehearsal], display), true);
  assert.deepEqual(steppedDown(rehearsal).role, { Audience: { primary: false } });
});

test("a window never takes the main place from anything", () => {
  assert.equal(takesMainPlace([output("Rehearsal", win, true)], win), false);
  assert.equal(takesMainPlace([output("Room", display, true)], win), false);
});

test("a display already holding the main place keeps it against another display", () => {
  const room = output("Room", display, true);
  assert.equal(windowToDemote([room], display), null);
  assert.equal(takesMainPlace([room], display), false);
});

test("a window that is not the main screen is left alone", () => {
  const rehearsal = output("Rehearsal", win, false);
  assert.equal(windowToDemote([rehearsal], display), null);
  assert.equal(takesMainPlace([rehearsal], display), true);
});
