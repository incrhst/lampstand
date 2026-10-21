// node --test --experimental-strip-types (npm test)
import { test } from "node:test";
import assert from "node:assert/strict";
import { jumpItem, nextClick, nextSlide } from "./cursor.ts";
import type { LoadedService } from "./api";

const slide = (label: string, steps = 0) => ({ label, lines: [label], steps, notes: "" });
const loaded: LoadedService = {
  service: "s",
  title: "Sunday",
  items: [
    { id: "a", title: "Amazing Grace", slides: [slide("V1"), slide("V2")] },
    { id: "h", title: "Welcome", slides: [] },
    { id: "b", title: "Be Thou My Vision", slides: [slide("V1")] },
  ],
};

test("with nothing live, next is the first slide", () => {
  assert.deepEqual(nextSlide(loaded, null)?.cursor, { item: "a", slide: 0, step: 0 });
});

test("next is the following slide of the live item", () => {
  const next = nextSlide(loaded, { item: "a", slide: 0, step: 0 });
  assert.equal(next?.slide.label, "V2");
  assert.equal(next?.title, "Amazing Grace");
});

test("after an item's last slide, next skips items with nothing to show", () => {
  assert.deepEqual(nextSlide(loaded, { item: "a", slide: 1, step: 0 })?.cursor, { item: "b", slide: 0, step: 0 });
});

test("after the last slide there is no next", () => {
  assert.equal(nextSlide(loaded, { item: "b", slide: 0, step: 0 }), null);
});

test("the next click builds the live slide before moving on", () => {
  const deck: LoadedService = {
    ...loaded,
    items: [{ id: "d", title: "Sermon", slides: [slide("Slide 1", 2), slide("Slide 2")] }, ...loaded.items],
  };
  const at = (step: number) => nextClick(deck, { item: "d", slide: 0, step });
  assert.deepEqual(at(0), { cursor: { item: "d", slide: 0, step: 1 }, slide: deck.items[0].slides[0], title: "Sermon", build: true });
  assert.equal(at(1)?.cursor.step, 2);
  assert.deepEqual(at(2)?.cursor, { item: "d", slide: 1, step: 0 });
  assert.equal(at(2)?.build, false);
  assert.deepEqual(nextClick(deck, { item: "d", slide: 1, step: 0 })?.cursor, { item: "a", slide: 0, step: 0 });
  assert.equal(nextClick(deck, null)?.cursor.item, "d");
});

test("down jumps to the start of the next item, from any slide, skipping empty items", () => {
  assert.deepEqual(jumpItem(loaded, { item: "a", slide: 0, step: 0 }, 1), { item: "b", slide: 0, step: 0 });
  assert.deepEqual(jumpItem(loaded, { item: "a", slide: 1, step: 2 }, 1), { item: "b", slide: 0, step: 0 });
});

test("up jumps to the start of the previous item, not back to this item's first slide", () => {
  assert.deepEqual(jumpItem(loaded, { item: "b", slide: 0, step: 0 }, -1), { item: "a", slide: 0, step: 0 });
  assert.deepEqual(jumpItem(loaded, { item: "b", slide: 0, step: 1 }, -1), { item: "a", slide: 0, step: 0 });
});

test("there is nowhere to jump past either end", () => {
  assert.equal(jumpItem(loaded, { item: "b", slide: 0, step: 0 }, 1), null);
  assert.equal(jumpItem(loaded, { item: "a", slide: 0, step: 0 }, -1), null);
});

test("with nothing live, down starts the service and up does nothing", () => {
  assert.deepEqual(jumpItem(loaded, null, 1), { item: "a", slide: 0, step: 0 });
  assert.equal(jumpItem(loaded, null, -1), null);
});
