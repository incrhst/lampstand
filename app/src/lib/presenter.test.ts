// node --test --experimental-strip-types (npm test)
import { test } from "node:test";
import assert from "node:assert/strict";
import { buildCounter, formatElapsed, slideCounter } from "./presenter.ts";

test("the counter is 1-based", () => {
  assert.equal(slideCounter(2, 8), "Slide 3 of 8");
});

test("builds count clicks, and say how many are left before the first", () => {
  assert.equal(buildCounter(0, 0), null);
  assert.equal(buildCounter(0, 3), "3 builds to go");
  assert.equal(buildCounter(0, 1), "1 build to go");
  assert.equal(buildCounter(2, 3), "Build 2 of 3");
});

test("elapsed time reads as a clock", () => {
  assert.equal(formatElapsed(0), "0:00");
  assert.equal(formatElapsed(7_900), "0:07");
  assert.equal(formatElapsed(754_000), "12:34");
  assert.equal(formatElapsed(3_723_000), "1:02:03");
  assert.equal(formatElapsed(-5), "0:00");
});
