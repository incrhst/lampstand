// node --test --experimental-strip-types (npm test)
import { test } from "node:test";
import assert from "node:assert/strict";
import { parseLabel, parseOrder, sectionCursor, unknownLabels } from "./sections.ts";
import type { LoadedService } from "./api";

const slide = (label: string) => ({ label, lines: [label], steps: 0, notes: "" });

// V1 C V2 C B C, with the first chorus split over two slides.
const service: LoadedService = {
  service: "s",
  title: "Sunday",
  sections: [],
  items: [
    { id: "header", title: "Worship", slides: [] },
    {
      id: "song",
      title: "Song",
      slides: ["V1", "C", "C", "V2", "C", "B", "C"].map(slide),
    },
  ],
};

test("labels parse like SectionKind::from_label", () => {
  assert.deepEqual(parseLabel("V2"), { kind: "verse", number: 2 });
  assert.deepEqual(parseLabel("Verse 3"), { kind: "verse", number: 3 });
  assert.deepEqual(parseLabel("Pre-Chorus"), { kind: "prechorus", number: null });
  assert.deepEqual(parseLabel("John 3:16"), { kind: "other", number: 16 });
});

test("number keys jump to that verse", () => {
  assert.deepEqual(sectionCursor(service, null, "1"), { item: "song", slide: 0, step: 0 });
  assert.deepEqual(sectionCursor(service, null, "2"), { item: "song", slide: 3, step: 0 });
  assert.equal(sectionCursor(service, null, "3"), null);
});

test("C finds the next chorus after the live slide, wrapping", () => {
  const at = (slide: number) => ({ item: "song", slide, step: 0 });
  assert.equal(sectionCursor(service, at(0), "c")?.slide, 1);
  // The second slide of the first chorus is not a new chorus.
  assert.equal(sectionCursor(service, at(1), "c")?.slide, 4);
  assert.equal(sectionCursor(service, at(5), "C")?.slide, 6);
  assert.equal(sectionCursor(service, at(6), "c")?.slide, 1);
  assert.equal(sectionCursor(service, at(0), "b")?.slide, 5);
});

test("other keys are not section keys", () => {
  assert.equal(sectionCursor(service, null, "x"), null);
  assert.equal(sectionCursor(service, null, "0"), null);
});

test("a typed order splits on spaces and commas", () => {
  assert.deepEqual(parseOrder("v1 v1, c  v2,v1"), ["v1", "v1", "c", "v2", "v1"]);
  assert.deepEqual(parseOrder(" , "), []);
});

test("labels that match no section are listed, ignoring case", () => {
  assert.deepEqual(unknownLabels(["v1", "C", "b", "V9"], ["V1", "C", "V2"]), ["b", "V9"]);
  assert.deepEqual(unknownLabels([], ["V1"]), []);
});
