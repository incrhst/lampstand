import { test } from "node:test";
import assert from "node:assert/strict";
import { END_OF_CHAPTER as END, formatPassage } from "./passage.ts";

const p = (book: number, [c1, v1]: [number, number], [c2, v2]: [number, number]) => ({
  book,
  start: { chapter: c1, verse: v1 },
  end: { chapter: c2, verse: v2 },
});

test("passages read as the core displays them", () => {
  assert.equal(formatPassage(p(43, [3, 16], [3, 16])), "John 3:16");
  assert.equal(formatPassage(p(43, [3, 16], [3, 18])), "John 3:16–18");
  assert.equal(formatPassage(p(43, [3, 16], [3, END])), "John 3:16–end");
  assert.equal(formatPassage(p(1, [1, 1], [2, 3])), "Genesis 1:1–2:3");
  assert.equal(formatPassage(p(19, [23, 1], [23, END])), "Psalms 23");
  assert.equal(formatPassage(p(19, [1, 1], [2, END])), "Psalms 1–2");
});

test("single-chapter books drop the chapter", () => {
  assert.equal(formatPassage(p(65, [1, 3], [1, 3])), "Jude 3");
  assert.equal(formatPassage(p(65, [1, 3], [1, 5])), "Jude 3–5");
  assert.equal(formatPassage(p(57, [1, 1], [1, END])), "Philemon");
});
