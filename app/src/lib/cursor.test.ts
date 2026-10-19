// node --test --experimental-strip-types (npm test)
import { test } from "node:test";
import assert from "node:assert/strict";
import { nextSlide } from "./cursor.ts";
import type { LoadedService } from "./api";

const slide = (label: string) => ({ label, lines: [label], steps: 0 });
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
