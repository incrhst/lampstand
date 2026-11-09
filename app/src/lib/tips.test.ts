// node --test --experimental-strip-types (npm test)
import { test } from "node:test";
import assert from "node:assert/strict";
import { connectDue, nextTip, snooze, TIPS } from "./tips.ts";
import type { LoadedService } from "./api";

const service = (
  sections: { header: string; seconds: number | null; items: string[] }[],
  slides: Record<string, number> = {},
): LoadedService => ({
  service: "s",
  title: "Sunday",
  // Only the slide counts matter to the tips.
  items: Object.entries(slides).map(([id, n]) => ({ id, slides: Array.from({ length: n }) })) as unknown as LoadedService["items"],
  sections: sections.map((s) => ({ ...s, title: s.header })),
});

const ALL_PRESENT = ["quick-add", "jump-keys", "blank-screen", "safe-area", "loop-progress"];

test("nothing is offered when no service is open", () => {
  assert.equal(nextTip({ loaded: null, liveItem: null }, new Set()), null);
});

test("a service with a group offers the loop tip", () => {
  const f = { loaded: service([{ header: "h", seconds: null, items: ["a"] }]), liveItem: null };
  assert.equal(nextTip(f, new Set())?.id, "loop-group");
});

test("a tip for a feature already used is not offered", () => {
  // The group already loops, so Loop a group has nothing to teach.
  const f = { loaded: service([{ header: "h", seconds: 8, items: ["a"] }]), liveItem: null };
  assert.notEqual(nextTip(f, new Set())?.id, "loop-group");
});

test("the progress tip comes before the service once a group loops", () => {
  const f = { loaded: service([{ header: "h", seconds: 8, items: ["a"] }]), liveItem: null };
  assert.equal(nextTip(f, new Set())?.id, "loop-progress");
});

test("the fold tip needs two groups with items", () => {
  const one = { loaded: service([{ header: "h", seconds: 8, items: ["a"] }]), liveItem: null };
  assert.notEqual(nextTip(one, new Set(["loop-group"]))?.id, "fold-group");
  const two = {
    loaded: service([
      { header: "h", seconds: null, items: ["a"] },
      { header: "g", seconds: null, items: ["b"] },
    ]),
    liveItem: null,
  };
  assert.equal(nextTip(two, new Set(["loop-group"]))?.id, "fold-group");
});

test("a dismissed tip stays away", () => {
  const f = { loaded: service([{ header: "h", seconds: null, items: ["a"] }]), liveItem: null };
  assert.notEqual(nextTip(f, new Set(["loop-group"]))?.id, "loop-group");
});

test("tip ids are unique", () => {
  assert.equal(new Set(TIPS.map((t) => t.id)).size, TIPS.length);
});

test("tips about Present come before the service, one at a time", () => {
  const f = { loaded: service([], { a: 4 }), liveItem: null };
  assert.equal(nextTip(f, new Set())?.id, "quick-add");
  assert.equal(nextTip(f, new Set(["quick-add"]))?.id, "jump-keys");
  assert.equal(nextTip(f, new Set(["quick-add", "jump-keys"]))?.id, "blank-screen");
  assert.equal(nextTip(f, new Set(["quick-add", "jump-keys", "blank-screen"]))?.id, "safe-area");
});

test("no tip is offered while something is on air", () => {
  const f = { loaded: service([{ header: "h", seconds: 8, items: ["a"] }], { a: 4 }), liveItem: "a", hasLogo: true, paired: 0, realUse: true };
  assert.equal(nextTip(f, new Set(["loop-group", "fold-group"])), null);
});

test("the jump-keys tip needs an item with three or more slides", () => {
  const f = { loaded: service([], { a: 2 }), liveItem: null };
  assert.equal(nextTip(f, new Set(["quick-add"]))?.id, "blank-screen");
});

test("the logo-bug tip waits for a logo", () => {
  const f = { loaded: service([], { a: 1 }), liveItem: null };
  const seen = new Set(ALL_PRESENT);
  assert.equal(nextTip(f, seen), null);
  assert.equal(nextTip({ ...f, hasLogo: true }, seen)?.id, "logo-bug");
});

test("there are nine tips", () => {
  assert.equal(TIPS.length, 9);
});

const real = { paired: 0, realUse: true };

test("connect-a-device is offered with nothing paired once the church is using Lampstand for real", () => {
  assert.equal(nextTip({ loaded: null, liveItem: null, ...real }, new Set())?.id, "connect-device");
  assert.equal(TIPS.find((t) => t.id === "connect-device")?.action?.go, "devices");
});

test("connect-a-device stays away during the welcome-only state or before sync has answered", () => {
  assert.equal(nextTip({ loaded: null, liveItem: null, paired: 0, realUse: false }, new Set()), null);
  assert.equal(nextTip({ loaded: null, liveItem: null, realUse: true }, new Set()), null);
});

test("connect-a-device is not shown once a device is paired", () => {
  assert.equal(nextTip({ loaded: null, liveItem: null, paired: 1, realUse: true }, new Set()), null);
});

test("connect-a-device is never offered while something is on air", () => {
  assert.equal(nextTip({ loaded: null, liveItem: "a", ...real }, new Set(["quick-add", "jump-keys", "blank-screen", "safe-area"]))?.id, undefined);
});

test("Not now puts connect-a-device off for a week, then it comes back", () => {
  const f = { loaded: null, liveItem: null, ...real };
  const now = 1_000_000;
  const s = snooze({}, "connect-device", 7, now);
  assert.equal(nextTip(f, new Set(), TIPS, s, now + 6 * 86_400_000), null);
  assert.equal(nextTip(f, new Set(), TIPS, s, now + 7 * 86_400_000)?.id, "connect-device");
});

test("dismissing connect-a-device hides it for good, and the status line follows", () => {
  const f = { loaded: null, liveItem: null, ...real };
  assert.equal(nextTip(f, new Set(["connect-device"])), null);
  assert.equal(connectDue(real, new Set(), {}), true);
  assert.equal(connectDue(real, new Set(["connect-device"]), {}), false);
  assert.equal(connectDue(real, new Set(), snooze({}, "connect-device", 7)), false);
  assert.equal(connectDue({ paired: 2, realUse: true }, new Set(), {}), false);
});
