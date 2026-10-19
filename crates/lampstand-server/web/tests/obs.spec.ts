// T43: the OBS browser source draws what the GPU renderer draws.
//
// `cargo run --example obs_fixture` renders a few scenes with
// `render_offscreen` (the golden reference) and serves them on the real
// routes. Each test publishes a scene, loads the page the way OBS does and
// compares a screenshot (transparent background kept) with the reference.
//
// Without a GPU or software adapter (no lavapipe on a Linux box) the
// fixture says SKIP and every test here is skipped. CI installs lavapipe.

import { expect, test, type Page } from "@playwright/test";
import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { PNG } from "pngjs";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "../../../..");
const out = resolve(here, "../test-results/fixture");

/**
 * A pixel matches when no premultiplied channel differs by more than this
 * (of 255). Chromium and the GPU renderer anti-alias glyph edges
 * differently, and Chromium puts each line's baseline on a whole pixel, so
 * edge pixels differ by tens of levels; solid areas and blurs must agree.
 */
const CHANNEL_TOLERANCE = 64;
/**
 * At most this fraction of the pixels either image paints (shadows
 * included) may differ by more than that. Measured on a lyrics lower third:
 * 5.3%, all on glyph edges. A line that is misplaced by 10 px or wraps
 * differently is over 20%.
 */
const MAX_MISMATCHED = 0.08;

let child: ChildProcessWithoutNullStreams | undefined;
let base = "";
let lines: string[] = [];
let waiting: Array<() => void> = [];
let skipReason: string | undefined;
let output = "";
let key = "";

function nextLine(match: (l: string) => boolean, ms = 600_000): Promise<string> {
  return new Promise((resolveLine, reject) => {
    const timer = setTimeout(() => reject(new Error("fixture: timed out")), ms);
    const check = () => {
      const i = lines.findIndex(match);
      if (i < 0) return false;
      clearTimeout(timer);
      resolveLine(lines.splice(i, 1)[0]);
      return true;
    };
    if (!check()) waiting.push(() => check());
  });
}

test.beforeAll(async () => {
  test.setTimeout(900_000);
  mkdirSync(out, { recursive: true });
  const bin = process.env.LAMPSTAND_OBS_FIXTURE;
  child = bin
    ? spawn(bin, [out], { cwd: root })
    : spawn("cargo", ["run", "-q", "-p", "lampstand-server", "--example", "obs_fixture", "--", out], {
        cwd: root,
      });
  let buffer = "";
  child.stdout.on("data", (d) => {
    buffer += String(d);
    const parts = buffer.split("\n");
    buffer = parts.pop() ?? "";
    lines.push(...parts);
    waiting = waiting.filter((w) => (w(), false));
  });
  child.stderr.on("data", (d) => process.stderr.write(d));
  const hello = await nextLine((l) => l.startsWith("READY") || l.startsWith("SKIP"));
  if (hello.startsWith("SKIP")) {
    skipReason = hello;
    return;
  }
  const [, port, o, k] = hello.split(" ");
  base = `http://127.0.0.1:${port}`;
  output = o;
  key = k;
});

test.afterAll(() => {
  child?.kill();
});

test.beforeEach(() => {
  test.skip(skipReason !== undefined, skipReason);
});

async function show(page: Page, name: string): Promise<Buffer> {
  child!.stdin.write(`show ${name}\n`);
  await nextLine((l) => l === `SHOWN ${name}`, 30_000);
  await page.goto(`${base}/obs/${output}?k=${key}`);
  // The page sets this once the scene's fonts and images are in and drawn.
  await page.waitForFunction(() => document.documentElement.dataset.seq !== undefined);
  await page.evaluate(
    () => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))),
  );
  return page.screenshot({ omitBackground: true });
}

function reference(name: string): PNG {
  return PNG.sync.read(readFileSync(join(out, `${name}.png`)));
}

function premultiplied(png: PNG, i: number): [number, number, number, number] {
  const a = png.data[i + 3];
  return [
    (png.data[i] * a) / 255,
    (png.data[i + 1] * a) / 255,
    (png.data[i + 2] * a) / 255,
    a,
  ];
}

/** Fraction of painted pixels that differ, and the worst fraction's diff image. */
function compare(name: string, shot: Buffer, tolerance: number, against = name) {
  const actual = PNG.sync.read(shot);
  const expected = reference(against);
  expect([actual.width, actual.height]).toEqual([expected.width, expected.height]);
  const diff = new PNG({ width: actual.width, height: actual.height });
  let painted = 0;
  let mismatched = 0;
  for (let i = 0; i < actual.data.length; i += 4) {
    const a = premultiplied(actual, i);
    const e = premultiplied(expected, i);
    const worst = Math.max(...a.map((v, c) => Math.abs(v - e[c])));
    const isPainted = a[3] > 0 || e[3] > 0;
    if (isPainted) painted++;
    const bad = worst > tolerance;
    if (bad) mismatched++;
    diff.data[i] = bad ? 255 : 0;
    diff.data[i + 1] = 0;
    diff.data[i + 2] = 0;
    diff.data[i + 3] = bad ? 255 : isPainted ? 60 : 0;
  }
  writeFileSync(join(out, `${name}.actual.png`), shot);
  writeFileSync(join(out, `${name}.diff.png`), PNG.sync.write(diff));
  return { painted, mismatched, rate: painted === 0 ? 0 : mismatched / painted };
}

test("a lyrics lower third matches render_offscreen", async ({ page }) => {
  const shot = await show(page, "lyrics");
  const { painted, rate } = compare("lyrics", shot, CHANNEL_TOLERANCE);
  console.log(`lyrics: ${painted} painted pixels, ${(rate * 100).toFixed(2)}% mismatched`);
  expect(painted).toBeGreaterThan(10_000);
  expect(rate).toBeLessThanOrEqual(MAX_MISMATCHED);
  // The tolerance still tells scenes apart.
  expect(compare("lyrics", shot, CHANNEL_TOLERANCE, "plate").rate).toBeGreaterThan(0.5);
});

test("a plate with gradient, dash, shadows, bullets and opacity matches", async ({ page }) => {
  const shot = await show(page, "plate");
  const { painted, rate } = compare("plate", shot, CHANNEL_TOLERANCE);
  console.log(`plate: ${painted} painted pixels, ${(rate * 100).toFixed(2)}% mismatched`);
  expect(rate).toBeLessThanOrEqual(MAX_MISMATCHED);
});

test("a deck slide arrives as a frame and matches exactly", async ({ page }) => {
  const shot = await show(page, "deck");
  expect(await page.locator("#stage img").count()).toBe(1);
  const { rate } = compare("deck", shot, 2);
  expect(rate).toBe(0);
});

test("unpainted pixels are fully transparent", async ({ page }) => {
  // Nothing on air: every pixel of the screenshot has alpha 0.
  const empty = PNG.sync.read(await show(page, "empty"));
  for (let i = 3; i < empty.data.length; i += 4) {
    if (empty.data[i] !== 0) throw new Error(`pixel ${(i - 3) / 4} is not transparent`);
  }
  const background = await page.evaluate(() => [
    getComputedStyle(document.documentElement).backgroundColor,
    getComputedStyle(document.body).backgroundColor,
  ]);
  expect(background).toEqual(["rgba(0, 0, 0, 0)", "rgba(0, 0, 0, 0)"]);

  // With a lower third up, the top of the frame (well clear of text and
  // its shadow) is still fully transparent.
  const lyrics = PNG.sync.read(await show(page, "lyrics"));
  for (let y = 0; y < 600; y++) {
    for (let x = 0; x < lyrics.width; x++) {
      if (lyrics.data[(y * lyrics.width + x) * 4 + 3] !== 0) {
        throw new Error(`pixel ${x},${y} is not transparent`);
      }
    }
  }
});

test("the page scales to the size OBS gives it", async ({ page }) => {
  await page.setViewportSize({ width: 960, height: 540 });
  const shot = PNG.sync.read(await show(page, "lyrics"));
  expect([shot.width, shot.height]).toEqual([960, 540]);
  // Text lands in the lower third, scaled.
  let top = shot.height;
  for (let y = 0; y < shot.height && top === shot.height; y++) {
    for (let x = 0; x < shot.width; x++) {
      if (shot.data[(y * shot.width + x) * 4 + 3] > 0) {
        top = y;
        break;
      }
    }
  }
  expect(top).toBeGreaterThan(540 * 0.6);
});
