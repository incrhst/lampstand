import { test } from "node:test";
import assert from "node:assert/strict";
import { wrapTab } from "./modal.ts";

test("Tab from the last control goes back to the first", () => {
  assert.equal(wrapTab(2, 3, false), 0);
});

test("Shift+Tab from the first control goes to the last", () => {
  assert.equal(wrapTab(0, 3, true), 2);
});

test("Tab between controls is left to the browser", () => {
  assert.equal(wrapTab(0, 3, false), null);
  assert.equal(wrapTab(2, 3, true), null);
});

test("focus outside the dialog comes back in at the near end", () => {
  assert.equal(wrapTab(-1, 3, false), 0);
  assert.equal(wrapTab(-1, 3, true), 2);
});

test("a dialog with no controls leaves Tab alone", () => {
  assert.equal(wrapTab(-1, 0, false), null);
});
