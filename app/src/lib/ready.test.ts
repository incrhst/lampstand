// node --test --experimental-strip-types (npm test)
import { test } from "node:test";
import assert from "node:assert/strict";
import { nextSlide } from "./cursor.ts";
import { isPinned, loadedItem, readyCount, readyList, returnLine } from "./ready.ts";
import type { LoadedService, Service } from "./api";

const slide = (label: string) => ({ label, lines: [label], footer: null, steps: 0, notes: "" });
const a = { id: "a", title: "Amazing Grace", slides: [slide("V1"), slide("V2")], sounds: [] };
const b = { id: "b", title: "Notices", slides: [slide("1")], sounds: [] };
const r = { id: "r", title: "Welcome video", slides: [slide("1"), slide("2")], sounds: [] };
const loaded: LoadedService = {
  service: "s",
  title: "Sunday",
  sections: [],
  items: [a, b],
  ready: [
    { item: b, in_order: true },
    { item: r, in_order: false },
    { item: { id: "x", title: "Missing deck", slides: [], sounds: [] }, in_order: false },
  ],
};

test("an item kept only in Ready is found beside the order", () => {
  assert.equal(loadedItem(loaded, "r")?.title, "Welcome video");
  assert.equal(loadedItem(loaded, "a")?.title, "Amazing Grace");
  assert.equal(loadedItem(loaded, "nope"), undefined);
});

test("the Ready card leaves out what has nothing to show", () => {
  assert.deepEqual(readyList(loaded).map((x) => x.item.id), ["b", "r"]);
  assert.deepEqual(readyList({ ...loaded, ready: undefined }), []);
});

test("on a side-trip, next steps through the ready item and then goes back", () => {
  const back = { item: "a", slide: 1, step: 0 };
  assert.equal(nextSlide(loaded, { item: "r", slide: 0, step: 0 }, back)?.slide.label, "2");
  assert.deepEqual(nextSlide(loaded, { item: "r", slide: 1, step: 0 }, back)?.cursor, back);
});

test("with nowhere to go back to, a ready item's end starts the service", () => {
  assert.deepEqual(nextSlide(loaded, { item: "r", slide: 1, step: 0 })?.cursor, { item: "a", slide: 0, step: 0 });
});

test("the way back is said in words", () => {
  assert.equal(returnLine(loaded, { item: "a", slide: 1, step: 0 }), "Next returns to Amazing Grace · V2");
  assert.equal(returnLine(loaded, null), null);
});

test("a pin counts only while its item is in the order", () => {
  const service = {
    items: { a: {} },
    ready: { a: { position: "V", source: "Order" }, gone: { position: "W", source: "Order" }, own: { position: "X", source: { Own: { kind: { Deck: { deck: "d" } } } } } },
  } as unknown as Service;
  assert.equal(isPinned(service, "a"), true);
  assert.equal(isPinned(service, "own"), false);
  assert.equal(readyCount(service), 2);
  assert.equal(readyCount(null), 0);
});
