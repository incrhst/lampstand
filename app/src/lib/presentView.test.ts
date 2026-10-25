import { test } from "node:test";
import assert from "node:assert/strict";
import { slideMark } from "./presentView.ts";

test("a card is marked live, next, or neither", () => {
  const live = { item: "a", slide: 1, step: 0 };
  const next = { item: "a", slide: 2, step: 0 };
  assert.equal(slideMark("a", 1, live, next), "live");
  assert.equal(slideMark("a", 2, live, next), "next");
  assert.equal(slideMark("a", 0, live, next), null);
  assert.equal(slideMark("b", 1, live, next), null);
  assert.equal(slideMark("a", 1, null, null), null);
});
