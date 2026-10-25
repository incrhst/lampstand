import { test } from "node:test";
import assert from "node:assert/strict";
import type { ParsedSong } from "./api.ts";
import {
  blankDetails,
  effectiveTitle,
  previewRows,
  problem,
  sungOrder,
  titleFromText,
  toSong,
  words,
} from "./pasteSong.ts";

const parsed = (over: Partial<ParsedSong["song"]> = {}, rest: Partial<ParsedSong> = {}): ParsedSong => ({
  song: {
    id: "s1",
    title: "Amazing Grace",
    author: null,
    copyright: null,
    ccli_number: null,
    key: null,
    tempo: null,
    sections: [
      { label: "V1", lines: ["a", "b"] },
      { label: "C", lines: ["c"] },
    ],
    arrangement: [],
    ...over,
  },
  warnings: [],
  same_title: null,
  ...rest,
});

test("a typed title wins over the first line the paste had", () => {
  assert.equal(effectiveTitle("  My Title ", parsed()), "My Title");
  assert.equal(effectiveTitle("", parsed()), "Amazing Grace");
  assert.equal(effectiveTitle("", parsed({ title: "" })), "");
  assert.equal(effectiveTitle("", null), "");
});

test("the dialog says the title came from the first line only when it did", () => {
  assert.equal(titleFromText("", parsed()), "Amazing Grace");
  assert.equal(titleFromText("Mine", parsed()), null);
  assert.equal(titleFromText("", parsed({ title: "" })), null);
  assert.equal(words.titleFromText("Amazing Grace"), "Using “Amazing Grace” from the first line.");
});

test("a song cannot be added without a title or with a bad CCLI number", () => {
  const d = blankDetails();
  assert.equal(problem(d, null, null), null, "nothing pasted yet is not a complaint");
  assert.equal(problem(d, parsed({ title: "" }), null), "The song needs a title.");
  assert.equal(problem({ ...d, title: "X" }, parsed({ title: "" }), null), null);
  assert.equal(problem({ ...d, ccli: "12ab" }, parsed(), null), "The CCLI number should be digits only.");
  assert.equal(problem({ ...d, ccli: "99999999999" }, parsed(), null), "The CCLI number should be digits only.");
  assert.equal(problem({ ...d, ccli: "4294967296" }, parsed(), null), "The CCLI number is too long.");
  assert.equal(problem({ ...d, ccli: "4755360" }, parsed(), null), null);
  assert.equal(problem(d, parsed(), "Paste the words of a song."), "Paste the words of a song.");
});

test("the song saved is what the core read with what was typed beside it", () => {
  const p = parsed({ author: "From the page", ccli_number: 111 });
  const song = toSong(p, { title: " New ", author: "", copyright: "© 2020 Me", ccli: "222", key: " C " });
  assert.equal(song.title, "New");
  assert.equal(song.author, "From the page", "a blank box keeps what the page gave");
  assert.equal(song.copyright, "© 2020 Me");
  assert.equal(song.ccli_number, 222);
  assert.equal(song.key, "C");
  assert.deepEqual(song.sections, p.song.sections);
  assert.equal(song.id, "s1");
  const plain = toSong(parsed(), blankDetails());
  assert.equal(plain.ccli_number, null);
  assert.equal(plain.author, null);
});

test("the preview lists each section with its line count and first line", () => {
  assert.deepEqual(previewRows(parsed()), [
    { label: "V1", detail: "2 lines", first: "a" },
    { label: "C", detail: "1 line", first: "c" },
  ]);
  assert.deepEqual(previewRows(null), []);
});

test("the sung order is shown only when it differs from the order written", () => {
  assert.equal(sungOrder(parsed()), null);
  assert.deepEqual(sungOrder(parsed({ arrangement: ["V1", "C", "C"] })), ["V1", "C", "C"]);
  assert.equal(words.sung(["V1", "C", "C"]), "Sung in this order: V1, C, C");
  assert.equal(sungOrder(null), null);
});

test("the library warning names the song", () => {
  assert.equal(
    words.sameTitle("Amazing Grace"),
    "A song called “Amazing Grace” is already in the library. Adding this will make a second one.",
  );
  assert.equal(words.addToService("Sunday"), "Add to library and to “Sunday”");
});
