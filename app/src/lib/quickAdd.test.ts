// node --test --experimental-strip-types (npm test)
import { test } from "node:test";
import assert from "node:assert/strict";
import { entryAction, insertionIndex, newItemId } from "./quickAdd.ts";

const order = ["welcome", "song1", "sermon", "song2"];

test("next goes right after the item on air", () => {
  assert.equal(insertionIndex(order, "song1", "next"), 2);
  assert.equal(insertionIndex(order, "welcome", "next"), 1);
});

test("next after the last item is the end", () => {
  assert.equal(insertionIndex(order, "song2", "next"), 4);
});

test("with nothing on air, or the live item gone, an item joins the end", () => {
  assert.equal(insertionIndex(order, null, "next"), 4);
  assert.equal(insertionIndex(order, "deleted", "next"), 4);
});

test("end is always the end, whatever is on air", () => {
  assert.equal(insertionIndex(order, "welcome", "end"), 4);
});

test("the new item is the one that was not there before", () => {
  assert.equal(newItemId(["a", "b"], ["a", "x", "b"]), "x");
  assert.equal(newItemId(["a"], ["a"]), null);
});

test("Enter adds next, Shift+Enter adds next and shows it, Ctrl or Cmd+Enter adds at the end", () => {
  const key = (shiftKey: boolean, metaKey: boolean, ctrlKey: boolean) => entryAction({ shiftKey, metaKey, ctrlKey });
  assert.deepEqual(key(false, false, false), { where: "next", show: false });
  assert.deepEqual(key(true, false, false), { where: "next", show: true });
  assert.deepEqual(key(false, true, false), { where: "end", show: false });
  assert.deepEqual(key(false, false, true), { where: "end", show: false });
});
