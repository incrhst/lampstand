import { test } from "node:test";
import assert from "node:assert/strict";
import { formatWhen } from "./when.ts";

test("a service without a start time says so", () => {
  assert.equal(formatWhen(null), "No date set");
  assert.equal(formatWhen(undefined), "No date set");
});

test("a start time shows its day and time", () => {
  const at = new Date(2026, 9, 18, 10, 30).getTime();
  const text = formatWhen(at, "en-GB");
  assert.match(text, /18 Oct/);
  assert.match(text, /10:30/);
  assert.match(text, / · /);
});
