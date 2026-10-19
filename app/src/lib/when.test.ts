import { test } from "node:test";
import assert from "node:assert/strict";
import { comingSunday, formatWhen, fromInputs, toInputs, zonedTime } from "./when.ts";

const LONDON = "Europe/London";
const KINGSTON = "America/Jamaica";
const AUCKLAND = "Pacific/Auckland";

/** An instant as its wall-clock reading in `zone`, for readable asserts. */
const wall = (at: number, zone: string) => {
  const { date, time } = toInputs(at, zone);
  return `${date} ${time}`;
};

test("a service without a start time says so", () => {
  assert.equal(formatWhen(null), "No date set");
  assert.equal(formatWhen(undefined), "No date set");
});

test("a start time shows its day and time in the service's zone", () => {
  const at = zonedTime(2026, 10, 18, 10, 30, LONDON);
  const text = formatWhen(at, LONDON, "en-GB", LONDON);
  assert.match(text, /18 Oct/);
  assert.match(text, /10:30/);
  assert.match(text, / · /);
  assert.doesNotMatch(text, /GMT|BST/);
});

test("a viewer in another zone sees the church's time, named", () => {
  const at = zonedTime(2026, 10, 25, 10, 0, KINGSTON);
  const text = formatWhen(at, KINGSTON, "en-GB", LONDON);
  assert.match(text, /25 Oct · 10:00 /);
  assert.match(text, /GMT-5|EST/);
});

test("a service without a zone reads in the viewer's zone", () => {
  const at = Date.UTC(2026, 9, 25, 15, 0);
  assert.match(formatWhen(at, null, "en-GB", KINGSTON), /10:00$/);
});

test("zoned times convert both ways", () => {
  assert.equal(zonedTime(2026, 10, 25, 10, 0, KINGSTON), Date.UTC(2026, 9, 25, 15, 0));
  assert.equal(zonedTime(2026, 7, 5, 10, 0, LONDON), Date.UTC(2026, 6, 5, 9, 0));
  assert.equal(zonedTime(2026, 12, 6, 10, 0, LONDON), Date.UTC(2026, 11, 6, 10, 0));
  assert.equal(zonedTime(2026, 10, 25, 10, 0, AUCKLAND), Date.UTC(2026, 9, 24, 21, 0));
});

// In London, October 2026: Sunday the 18th, and Sunday the 25th, when the
// clocks go back an hour at 02:00.
const london = (day: number, hour = 0, minute = 0) => zonedTime(2026, 10, day, hour, minute, LONDON);

test("a new service defaults to the coming Sunday at 10:00", () => {
  assert.equal(wall(comingSunday(london(21, 15), LONDON), LONDON), "2026-10-25 10:00");
  assert.equal(wall(comingSunday(london(24, 23, 59), LONDON), LONDON), "2026-10-25 10:00");
  assert.equal(wall(comingSunday(london(19, 0, 1), LONDON), LONDON), "2026-10-25 10:00");
});

test("on Sunday it is today until 10:00, then next week", () => {
  assert.equal(wall(comingSunday(london(18, 8), LONDON), LONDON), "2026-10-18 10:00");
  assert.equal(wall(comingSunday(london(18, 9, 59), LONDON), LONDON), "2026-10-18 10:00");
  assert.equal(wall(comingSunday(london(18, 10), LONDON), LONDON), "2026-10-25 10:00");
  assert.equal(wall(comingSunday(london(18, 19), LONDON), LONDON), "2026-10-25 10:00");
});

test("the coming Sunday keeps 10:00 across a daylight-saving change", () => {
  // Saturday in summer time; Sunday is in winter time.
  const sunday = comingSunday(london(24, 12), LONDON);
  assert.equal(wall(sunday, LONDON), "2026-10-25 10:00");
  assert.equal(sunday, Date.UTC(2026, 9, 25, 10, 0));
});

test("the coming Sunday can cross into the next month", () => {
  assert.equal(wall(comingSunday(london(26, 9), LONDON), LONDON), "2026-11-01 10:00");
});

test("the coming Sunday is the church's Sunday, not the device's", () => {
  // Saturday 22:00 in Kingston is already Sunday 04:00 in London.
  const now = zonedTime(2026, 10, 24, 22, 0, KINGSTON);
  assert.equal(wall(comingSunday(now, KINGSTON), KINGSTON), "2026-10-25 10:00");
  assert.equal(wall(comingSunday(now, LONDON), LONDON), "2026-10-25 10:00");
  // Sunday 11:00 in Auckland is past its service; London's Sunday is ahead.
  const later = zonedTime(2026, 10, 25, 11, 0, AUCKLAND);
  assert.equal(wall(comingSunday(later, AUCKLAND), AUCKLAND), "2026-11-01 10:00");
  assert.equal(wall(comingSunday(later, LONDON), LONDON), "2026-10-25 10:00");
});

test("date and time inputs round-trip in a zone", () => {
  const at = zonedTime(2026, 10, 25, 9, 5, KINGSTON);
  assert.deepEqual(toInputs(at, KINGSTON), { date: "2026-10-25", time: "09:05" });
  assert.deepEqual(toInputs(at, LONDON), { date: "2026-10-25", time: "14:05" });
  assert.equal(fromInputs("2026-10-25", "09:05", KINGSTON), at);
});

test("a time repeated when the clocks go back is its first reading", () => {
  // 01:30 happens twice in London on 25 Oct 2026: BST first, then GMT.
  assert.equal(fromInputs("2026-10-25", "01:30", LONDON), Date.UTC(2026, 9, 25, 0, 30));
});

test("a time skipped when the clocks go forward moves past the gap", () => {
  // 01:30 does not happen in London on 29 Mar 2026; it reads as 02:30.
  assert.equal(wall(fromInputs("2026-03-29", "01:30", LONDON)!, LONDON), "2026-03-29 02:30");
});

test("an empty or bad date reads as no date, an empty time as 10:00", () => {
  assert.equal(fromInputs("", "10:00", LONDON), null);
  assert.equal(fromInputs("25/10/2026", "10:00", LONDON), null);
  assert.equal(fromInputs("2026-13-01", "10:00", LONDON), null);
  assert.equal(wall(fromInputs("2026-10-25", "", LONDON)!, LONDON), "2026-10-25 10:00");
});
