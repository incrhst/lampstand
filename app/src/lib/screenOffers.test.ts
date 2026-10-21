// node --test --experimental-strip-types (npm test)
import { test } from "node:test";
import assert from "node:assert/strict";
import { displaysWithScreens, newScreen, screenOffers } from "./screenOffers.ts";
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

const at = (name: string, x: number, width = 1920, height = 1080): DisplayMatch => ({ name, width, height, x, y: 0 });
const onDisplay = (name: string, d: DisplayMatch): OutputConfig => output({ name, target: { Display: d } });

test("a display with a screen is claimed, and one without is free", () => {
  const claimed = displaysWithScreens([onDisplay("TV", at("TV", 1440))], [at("TV", 1440), at("Projector", 3360, 1280, 720)]);
  assert.deepEqual([...claimed], [0]);
});

test("two displays of one size are told apart, so a second can still be added", () => {
  // Two identical projectors: the one screen set up claims the one at its position.
  const projectors = [at("", 1440), at("", 3360)];
  const claimed = displaysWithScreens([onDisplay("Left", at("", 1440))], projectors);
  assert.deepEqual([...claimed], [0]);
  const offers = screenOffers([onDisplay("Left", at("", 1440))], projectors);
  assert.deepEqual(offers.map((o) => o.label), ["Turn on Left", "Use display 2"]);
});

test("a second output for the same display claims nothing more", () => {
  const tv = at("Android TV", 1440);
  const claimed = displaysWithScreens([onDisplay("A", tv), onDisplay("B", tv)], [tv]);
  assert.deepEqual([...claimed], [0]);
});

test("a name match beats a size match", () => {
  // Two same-size displays; the output was made for the second by name.
  const claimed = displaysWithScreens([onDisplay("Epson", at("Epson", 3360))], [at("LG", 1440), at("Epson", 3360)]);
  assert.deepEqual([...claimed], [1]);
});
