import { test } from "node:test";
import assert from "node:assert/strict";
import { comingSunday, formatWhen, fromInputs, toInputs } from "./when.ts";

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

// October 2026: Sunday the 18th, Sunday the 25th.
const local = (day: number, hour = 0, minute = 0) => new Date(2026, 9, day, hour, minute);

test("a new service defaults to the coming Sunday at 10:00", () => {
  // Wednesday and Saturday night go to the next day that is Sunday.
  assert.deepEqual(comingSunday(local(21, 15)), local(25, 10));
  assert.deepEqual(comingSunday(local(24, 23, 59)), local(25, 10));
  // Monday goes six days on.
  assert.deepEqual(comingSunday(local(19, 0, 1)), local(25, 10));
});

test("on Sunday it is today until 10:00, then next week", () => {
  assert.deepEqual(comingSunday(local(18, 8)), local(18, 10));
  assert.deepEqual(comingSunday(local(18, 9, 59)), local(18, 10));
  assert.deepEqual(comingSunday(local(18, 10)), local(25, 10));
  assert.deepEqual(comingSunday(local(18, 19)), local(25, 10));
});

test("the coming Sunday can cross into the next month", () => {
  assert.deepEqual(comingSunday(new Date(2026, 9, 26, 9)), new Date(2026, 10, 1, 10));
});

test("date and time inputs round-trip a local time", () => {
  const at = local(25, 9, 5).getTime();
  assert.deepEqual(toInputs(at), { date: "2026-10-25", time: "09:05" });
  assert.equal(fromInputs("2026-10-25", "09:05"), at);
});

test("an empty or bad date reads as no date, an empty time as 10:00", () => {
  assert.equal(fromInputs("", "10:00"), null);
  assert.equal(fromInputs("25/10/2026", "10:00"), null);
  assert.equal(fromInputs("2026-10-25", ""), local(25, 10).getTime());
});
