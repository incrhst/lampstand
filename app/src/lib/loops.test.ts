import { test } from "node:test";
import assert from "node:assert/strict";
import type { ItemEstimate } from "./api.ts";
import {
  coveredAt,
  describeLine,
  estimatePass,
  formatLength,
  passLabel,
  sectionEnd,
  sectionOf,
  sectionSize,
  type Row,
} from "./loops.ts";

const row = (id: string, header = false): Row => ({ id, header, title: id });
const est = (id: string, over: Partial<ItemEstimate> = {}): ItemEstimate => ({
  item: id,
  stills: 0,
  click_slides: 0,
  video_ms: null,
  self_loop: false,
  open_ended: false,
  sound_ms: null,
  ...over,
});

const rows = [row("a"), row("h1", true), row("b"), row("c"), row("h2", true), row("d")];

  test("a section runs from its header to the next header", () => {
    assert.equal(sectionOf(rows, 3), 1);
    assert.equal(sectionOf(rows, 5), 4);
    assert.equal(sectionEnd(rows, 1), 4);
    assert.equal(sectionEnd(rows, 4), 6);
    assert.equal(sectionSize(rows, 1), 2);
  });

  test("rows before the first header are in no section", () => {
    assert.equal(sectionOf(rows, 0), -1);
  });

  test("counts what a new header would cover", () => {
    assert.equal(coveredAt(rows, 0), 1);
    assert.equal(coveredAt(rows, 2), 2);
    assert.equal(coveredAt(rows, 6), 0);
  });

const passRows = [row("slides"), row("video"), row("deck")];

  test("stills use the interval and a video plays at its own length", () => {
    const pass = estimatePass(
      passRows,
      {
        slides: est("slides", { stills: 6 }),
        video: est("video", { video_ms: 45_000 }),
        deck: est("deck", { stills: 4, click_slides: 2 }),
      },
      8,
    );
    assert.equal(pass.ms, 6 * 8000 + 45_000 + 4 * 8000);
    assert.equal(passLabel(pass), "≈ 2:05");
    assert.equal(pass.clickSlides, 2);
  });

  test("a different interval does not change a video", () => {
    const only = estimatePass([row("video")], { video: est("video", { video_ms: 45_000 }) }, 20);
    assert.equal(only.ms, 45_000);
  });

  test("a video that repeats itself or a countdown makes it a minimum", () => {
    const pass = estimatePass(
      [row("slides"), row("clip"), row("timer")],
      {
        slides: est("slides", { stills: 2 }),
        clip: est("clip", { video_ms: 9000, self_loop: true }),
        timer: est("timer", { open_ended: true }),
      },
      5,
    );
    assert.equal(pass.ms, 10_000);
    assert.equal(passLabel(pass), "At least 0:10");
    assert.deepEqual(pass.openEnded, ["clip", "timer"]);
    assert.deepEqual(pass.selfLoops.map((l) => l.id), ["clip"]);
  });

  test("describes each line", () => {
    const pass = estimatePass(passRows, { deck: est("deck", { stills: 4, click_slides: 2 }) }, 8);
    assert.equal(describeLine(pass.lines[0], 8), "4 slides × 8 s · 2 with click-only builds, shown finished");
  });

  test("formats lengths as minutes and seconds", () => {
    assert.equal(formatLength(87_000), "1:27");
    assert.equal(formatLength(5000), "0:05");
  });

test("music-only items add their length to a pass", () => {
  const pass = estimatePass(
    [row("slide"), row("walk-in")],
    { slide: est("slide", { stills: 2 }), "walk-in": est("walk-in", { sound_ms: 185_000 }) },
    10,
  );
  assert.equal(pass.ms, 2 * 10_000 + 185_000);
  assert.equal(formatLength(pass.ms), "3:25");
  assert.equal(passLabel(pass), "≈ 3:25");
  assert.equal(describeLine(pass.lines[1], 10), "music, 3:05");
});

test("a sound whose length is unknown makes the pass a minimum", () => {
  const pass = estimatePass([row("a")], { a: est("a", { sound_ms: null, open_ended: true }) }, 5);
  assert.equal(passLabel(pass), "At least 0:00");
});
