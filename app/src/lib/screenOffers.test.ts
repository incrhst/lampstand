// node --test --experimental-strip-types (npm test)
import { test } from "node:test";
import assert from "node:assert/strict";
import { newScreen, screenOffers } from "./screenOffers.ts";
import type { DisplayMatch, OutputConfig } from "./api";

const display = (name: string, width = 1920, height = 1080): DisplayMatch => ({ name, width, height, x: 0, y: 0 });

const output = (over: Partial<OutputConfig> & { name: string }): OutputConfig => ({
  id: over.name,
  role: { Audience: { primary: true } },
  target: { Window: { width: 960, height: 540 } },
  safe_area: { top: 0, right: 0, bottom: 0, left: 0 },
  background: "Bleed",
  theme: null,
  enabled: false,
  ...over,
});

test("a set-up screen that is off is offered, by name", () => {
  const offers = screenOffers([output({ name: "Rehearsal" })], []);
  assert.deepEqual(offers.map((o) => [o.kind, o.label]), [["enable", "Turn on Rehearsal"]]);
});

test("a display screen is offered only while its display is connected", () => {
  const s22 = output({ name: "S22F350", target: { Display: display("S22F350") } });
  assert.equal(screenOffers([s22], []).length, 0);
  assert.equal(screenOffers([s22], [display("S22F350")]).length, 1);
});

test("a display that already has a screen is not offered twice", () => {
  const s22 = output({ name: "S22F350", target: { Display: display("S22F350") } });
  const offers = screenOffers([s22], [display("S22F350")]);
  assert.deepEqual(offers.map((o) => o.kind), ["enable"]);
});

test("a connected display with no screen is offered as a new one", () => {
  const offers = screenOffers([], [display("S22F350"), display("")]);
  assert.deepEqual(offers.map((o) => o.label), ["Use S22F350", "Use display 2"]);
});

test("set-up screens come first, and there are at most three offers", () => {
  const outputs = [output({ name: "Rehearsal" })];
  const offers = screenOffers(outputs, [display("A", 1, 1), display("B", 2, 2), display("C", 3, 3)]);
  assert.equal(offers.length, 3);
  assert.equal(offers[0].kind, "enable");
});

test("screens that are already on, and stream outputs, are not offered", () => {
  const on = output({ name: "On", enabled: true });
  const stream = output({ name: "OBS", role: "Stream", target: { BrowserSource: { width: 1920, height: 1080 } } });
  assert.deepEqual(screenOffers([on, stream], []), []);
});

test("a display turned on becomes the main audience screen only if there is none", () => {
  const d = display("S22F350");
  const first = newScreen(d, [], "a");
  assert.deepEqual(first.role, { Audience: { primary: true } });
  assert.equal(first.enabled, true);
  const second = newScreen(d, [first], "b");
  assert.deepEqual(second.role, { Audience: { primary: false } });
});
