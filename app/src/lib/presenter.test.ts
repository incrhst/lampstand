// node --test --experimental-strip-types (npm test)
import { test } from "node:test";
import assert from "node:assert/strict";
import { stepCounter, formatElapsed, slideCounter } from "./presenter.ts";

test("the counter is 1-based", () => {
  assert.equal(slideCounter(2, 8), "Slide 3 of 8");
});

test("steps count clicks, and say how many the slide has before the first", () => {
  assert.equal(stepCounter(0, 0), null);
  assert.equal(stepCounter(0, 3), "3 steps on this slide");
  assert.equal(stepCounter(0, 1), "1 step on this slide");
  assert.equal(stepCounter(2, 3), "Step 2 of 3");
});

test("elapsed time reads as a clock", () => {
  assert.equal(formatElapsed(0), "0:00");
  assert.equal(formatElapsed(7_900), "0:07");
  assert.equal(formatElapsed(754_000), "12:34");
  assert.equal(formatElapsed(3_723_000), "1:02:03");
  assert.equal(formatElapsed(-5), "0:00");
});
