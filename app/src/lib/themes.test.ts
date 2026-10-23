import { test } from "node:test";
import assert from "node:assert/strict";
import type { Theme } from "./api/index.ts";
import {
  STARTER_THEME_ID,
  cannotDelete,
  contrast,
  copyName,
  defaultThemeId,
  duplicate,
  fromHex,
  makeTransition,
  readability,
  readableOn,
  sortThemes,
  textAreaId,
  themeFor,
  toHex,
  transitionDirection,
  transitionName,
} from "./themes.ts";

const theme = (id: string, name: string, over: Partial<Theme> = {}): Theme => ({
  id,
  name,
  text: {
    font_family: "Noto Sans",
    size: 72,
    min_size: 36,
    weight: 400,
    italic: false,
    color: { r: 255, g: 255, b: 255, a: 255 },
    align: "Center",
    valign: "Middle",
    line_spacing: 1.2,
    uppercase: false,
    shadow: null,
    outline: null,
  },
  background: { Color: { r: 0, g: 0, b: 0, a: 255 } },
  text_area: { x: 0, y: 0, width: 1, height: 1 },
  max_lines: 4,
  transition: { kind: "Cut", duration_ms: 0 },
  show_copyright: true,
  ...over,
});

const starter = theme(STARTER_THEME_ID, "Starter");

test("colours round-trip through hex", () => {
  assert.equal(toHex({ r: 20, g: 30, b: 90, a: 255 }), "#141e5a");
  assert.deepEqual(fromHex("#141E5A"), { r: 20, g: 30, b: 90, a: 255 });
  assert.deepEqual(fromHex("fff"), { r: 255, g: 255, b: 255, a: 255 });
  assert.equal(fromHex("#12"), null);
  assert.equal(fromHex("red"), null);
});

test("contrast and readable text match the core", () => {
  const black = { r: 0, g: 0, b: 0, a: 255 };
  const white = { r: 255, g: 255, b: 255, a: 255 };
  assert.ok(Math.abs(contrast(black, white) - 21) < 0.01);
  assert.deepEqual(readableOn({ r: 20, g: 30, b: 90, a: 255 }), white);
  assert.deepEqual(readableOn({ r: 250, g: 240, b: 200, a: 255 }), black);
});

test("hard-to-read text is flagged, and pictures are left alone", () => {
  assert.equal(readability(starter), null);
  const grey = theme("a", "Grey", { background: { Color: { r: 120, g: 120, b: 120, a: 255 } } });
  assert.match(readability(grey) ?? "", /hard to read/);
  const same = theme("b", "Same", { background: { Color: { r: 255, g: 255, b: 255, a: 255 } } });
  assert.match(readability(same) ?? "", /very hard/);
  assert.equal(readability(theme("c", "Photo", { background: { Image: "abc" } })), null);
});

test("a copy gets a new id and a name no theme has", () => {
  const all = [starter];
  assert.equal(copyName("Starter", all), "Starter copy");
  const copy = duplicate(starter, all, "new-id");
  assert.equal(copy.id, "new-id");
  assert.equal(copy.name, "Starter copy");
  assert.deepEqual(copy.text, starter.text);
  assert.notEqual(copy.text, starter.text, "a copy, not the same object");
  assert.equal(copyName("Starter", [...all, copy]), "Starter copy 2");
});

test("the Starter theme and the last theme cannot be deleted", () => {
  const mine = theme("m", "Mine");
  assert.match(cannotDelete(starter, [starter, mine]) ?? "", /Starter/);
  assert.equal(cannotDelete(mine, [starter, mine]), null);
  assert.match(cannotDelete(mine, [mine]) ?? "", /only theme/);
});

test("themes list the Starter first, then by name", () => {
  const list = sortThemes([theme("z", "Zion"), theme("a", "Abbey"), starter]);
  assert.deepEqual(
    list.map((t) => t.name),
    ["Starter", "Abbey", "Zion"],
  );
});

test("a screen's theme beats the service's, which beats the default, which beats the Starter", () => {
  const output = theme("o", "Screen");
  const service = theme("s", "Service");
  const dflt = theme("d", "Default");
  const all = [starter, output, service, dflt];
  const settings: { default_theme: string | null } = { default_theme: "d" };
  const pick = (o: string | null, s: string | null, set: { default_theme: string | null } = settings) => themeFor(all, set, { theme: o }, { theme: s })?.name;
  assert.equal(pick("o", "s"), "Screen");
  assert.equal(pick(null, "s"), "Service");
  assert.equal(pick(null, null), "Default");
  assert.equal(pick(null, null, { default_theme: null }), "Starter");
  assert.equal(pick("gone", "gone", { default_theme: "gone" }), "Starter", "a deleted theme falls through");
  assert.equal(defaultThemeId({ default_theme: null }, [dflt]), "d", "no Starter: any theme");
});

test("text areas match presets, else custom", () => {
  assert.equal(textAreaId({ x: 0, y: 0, width: 1, height: 1 }), "full");
  assert.equal(textAreaId({ x: 0, y: 0.7, width: 1, height: 0.3 }), "lower");
  assert.equal(textAreaId({ x: 0.1, y: 0.1, width: 0.5, height: 0.5 }), "custom");
});

test("transitions keep their side only when they have one", () => {
  assert.deepEqual(makeTransition("Cut", "Left", 400), { kind: "Cut", duration_ms: 0 });
  assert.deepEqual(makeTransition("Fade", "Left", 400), { kind: "Fade", duration_ms: 400 });
  const push = makeTransition("Push", "Down", 300);
  assert.deepEqual(push.kind, { Push: "Down" });
  assert.equal(transitionName(push.kind), "Push");
  assert.equal(transitionDirection(push.kind), "Down");
  assert.equal(transitionName("Zoom"), "Zoom");
});

test("a brand makes the Starter theme in its colours and font", async () => {
  const { themeFromBrand } = await import("./themes.ts");
  const brand = {
    name: " Grace Chapel ",
    logo: null,
    primary: { r: 20, g: 30, b: 90, a: 255 },
    text: null,
    accent: { r: 240, g: 190, b: 60, a: 255 },
    font_family: "Noto Serif",
  };
  const t = themeFromBrand(brand, starter);
  assert.equal(t.name, "Grace Chapel");
  assert.deepEqual(t.background, { Color: brand.primary });
  assert.equal(t.text.font_family, "Noto Serif");
  assert.deepEqual(t.text.color, { r: 255, g: 255, b: 255, a: 255 }, "readable on a dark colour");
  const plain = themeFromBrand({ ...brand, name: "", font_family: "", text: { r: 255, g: 230, b: 0, a: 255 } }, starter);
  assert.equal(plain.name, "Our church");
  assert.equal(plain.text.font_family, "Noto Sans");
  assert.deepEqual(plain.text.color, { r: 255, g: 230, b: 0, a: 255 });
  assert.equal(starter.name, "Starter", "the Starter theme is left alone");
});
