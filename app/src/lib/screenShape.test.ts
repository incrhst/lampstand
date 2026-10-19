// node --test --experimental-strip-types (npm test)
import { test } from "node:test";
import assert from "node:assert/strict";
import { marginsForScreen } from "./screenShape.ts";

const close = (a: number, b: number) => assert.ok(Math.abs(a - b) < 1e-9, `${a} is not ${b}`);

test("a 4:5 screen under a 16:9 projector keeps the middle 45% of the width", () => {
  const m = marginsForScreen(1920, 1080, 4 / 5);
  close(m.left, 0.275);
  close(m.right, 0.275);
  close(m.top, 0);
  close(m.bottom, 0);
});

test("a screen wider than the picture gets top and bottom margins", () => {
  const m = marginsForScreen(1024, 768, 16 / 9);
  close(m.top, 0.125);
  close(m.bottom, 0.125);
  close(m.left, 0);
});

test("the whole picture needs no margins", () => {
  assert.deepEqual(marginsForScreen(1920, 1080, null), { top: 0, right: 0, bottom: 0, left: 0 });
});
