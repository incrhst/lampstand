// node --test --experimental-strip-types (npm test)
import { test } from "node:test";
import assert from "node:assert/strict";
import { allDone, remaining, steps, welcomeService, WELCOME_TITLE, type Facts } from "./getStarted.ts";

const fresh: Facts = {
  screensOn: 0,
  songs: 2,
  services: [{ id: "w", title: WELCOME_TITLE, items: 5 }],
  triedService: false,
};
const done = (f: Facts) => steps(f).map((s) => [s.id, s.done]);

test("a brand-new install has nothing done", () => {
  assert.deepEqual(done(fresh), [["try", false], ["screen", false], ["songs", false], ["plan", false], ["look", false]]);
  assert.equal(remaining(steps(fresh)), 4);
  assert.equal(allDone(steps(fresh)), false);
});

test("each step ticks off by what the app can see", () => {
  assert.equal(done({ ...fresh, triedService: true })[0][1], true);
  assert.equal(done({ ...fresh, screensOn: 1 })[1][1], true);
  assert.equal(done({ ...fresh, songs: 3 })[2][1], true, "a song beyond the two that come with the app");
  assert.equal(done({ ...fresh, songs: 2 })[2][1], false);
});

test("the welcome service does not count as the church's own plan, and an empty service does not either", () => {
  assert.equal(done(fresh)[3][1], false);
  assert.equal(done({ ...fresh, services: [...fresh.services, { id: "s", title: "Sunday", items: 0 }] })[3][1], false);
  assert.equal(done({ ...fresh, services: [...fresh.services, { id: "s", title: "Sunday", items: 4 }] })[3][1], true);
});

test("everything done is noticed", () => {
  const all: Facts = { screensOn: 1, songs: 10, services: [{ id: "s", title: "Sunday", items: 4 }], triedService: true };
  assert.equal(allDone(steps(all)), true);
  assert.equal(remaining(steps(all)), 0);
});

test("the welcome service is found by name, and only while it has something in it", () => {
  assert.equal(welcomeService(fresh.services)?.id, "w");
  assert.equal(welcomeService([{ id: "w", title: WELCOME_TITLE, items: 0 }]), undefined);
  assert.equal(welcomeService([{ id: "x", title: "Sunday", items: 3 }]), undefined);
});

test("the church's look is offered but never holds the strip open", () => {
  const all: Facts = { screensOn: 1, songs: 10, services: [{ id: "s", title: "Sunday", items: 4 }], triedService: true };
  assert.equal(done(all).at(-1)![1], false);
  assert.equal(allDone(steps(all)), true, "the optional step is left undone");
  assert.equal(done({ ...all, brandSet: true }).at(-1)![1], true);
  assert.equal(remaining(steps(fresh)), 4, "optional steps are not counted as left to do");
});
