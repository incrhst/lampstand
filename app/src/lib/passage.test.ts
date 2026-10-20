import { test } from "node:test";
import assert from "node:assert/strict";
import { END_OF_CHAPTER as END, formatPassage, parseReference } from "./passage.ts";

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

const ref = (input: string) => parseReference(input);

test("references parse as the core parses them", () => {
  // The cases from `parses_common_references` in scripture.rs.
  const cases: [string, ReturnType<typeof p>][] = [
    ["Jn 3:16-18", p(43, [3, 16], [3, 18])],
    ["john 3:16", p(43, [3, 16], [3, 16])],
    ["John 3.16", p(43, [3, 16], [3, 16])],
    ["1 Cor 13:4-7", p(46, [13, 4], [13, 7])],
    ["1Co 13:4–7", p(46, [13, 4], [13, 7])],
    ["I Corinthians 13:4—7", p(46, [13, 4], [13, 7])],
    ["First Corinthians 13", p(46, [13, 1], [13, END])],
    ["Psalm 23", p(19, [23, 1], [23, END])],
    ["Ps. 119:105", p(19, [119, 105], [119, 105])],
    ["Gen 1:1-2:3", p(1, [1, 1], [2, 3])],
    ["Romans 8-9", p(45, [8, 1], [9, END])],
    ["Jude 3", p(65, [1, 3], [1, 3])],
    ["Jude 3-5", p(65, [1, 3], [1, 5])],
    ["Phil 4:13", p(50, [4, 13], [4, 13])],
    ["Phm 6", p(57, [1, 6], [1, 6])],
    ["Song of Songs 2:4", p(22, [2, 4], [2, 4])],
    ["  rev 22:21 ", p(66, [22, 21], [22, 21])],
    ["Rom 8:28", p(45, [8, 28], [8, 28])],
    ["1 Cor 13", p(46, [13, 1], [13, END])],
    ["Ps 23", p(19, [23, 1], [23, END])],
  ];
  for (const [input, expected] of cases) assert.deepEqual(ref(input), expected, input);
});

test("song titles and bad references are not references", () => {
  for (const input of ["", "Hezekiah 1:1", "J 3:16", "John 3:18-16", "John three", "Amazing grace", "10,000 Reasons", "How great", "I am 3"]) {
    assert.equal(ref(input), null, input);
  }
});
