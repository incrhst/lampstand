// node --test --experimental-strip-types (npm test)
import { test } from "node:test";
import assert from "node:assert/strict";
import { lines, moved, nextLabel, problems, toDraft, toSong } from "./songEdit.ts";
import type { Song } from "./api";

const song: Song = {
  id: "s1",
  title: "Amazing Grace",
  author: "John Newton",
  copyright: null,
  ccli_number: 22025,
  key: "G",
  tempo: 72,
  sections: [
    { label: "V1", lines: ["[G]Amazing [G7]grace", "how sweet the sound"] },
    { label: "C", lines: ["Chorus"] },
  ],
  arrangement: ["V1", "C", "V1"],
};

test("a draft saves back to the same song", () => {
  assert.deepEqual(toSong(song, toDraft(song)), song);
});

test("chord markup in lines is kept as typed", () => {
  const draft = toDraft(song);
  draft.sections[0].text = "[G]Amazing  [G7]grace \n [Em]how sweet";
  assert.deepEqual(toSong(song, draft).sections[0].lines, ["[G]Amazing  [G7]grace ", " [Em]how sweet"]);
});

test("blank fields become null and numbers are read", () => {
  const draft = toDraft(song);
  Object.assign(draft, { author: "  ", ccli: " 1234 ", tempo: "", key: " Bb " });
  const saved = toSong(song, draft);
  assert.equal(saved.author, null);
  assert.equal(saved.ccli_number, 1234);
  assert.equal(saved.tempo, null);
  assert.equal(saved.key, "Bb");
});

test("problems names what stops a save", () => {
  const draft = toDraft(song);
  assert.deepEqual(problems(draft), []);
  Object.assign(draft, { title: " ", ccli: "12a", tempo: "99999" });
  draft.sections[1].label = " ";
  assert.equal(problems(draft).length, 4);
});

test("trailing blank lines are dropped but inner ones stay", () => {
  assert.deepEqual(lines("a\n\nb\n\n  \n"), ["a", "", "b"]);
  assert.deepEqual(lines(""), []);
});

test("moved shifts one item and stops at the ends", () => {
  assert.deepEqual(moved(["a", "b", "c"], 0, 1), ["b", "a", "c"]);
  assert.deepEqual(moved(["a", "b", "c"], 2, -2), ["c", "a", "b"]);
  assert.deepEqual(moved(["a", "b", "c"], 0, -1), ["a", "b", "c"]);
});

test("nextLabel continues the verses", () => {
  assert.equal(nextLabel([]), "V1");
  assert.equal(nextLabel(["V1", "C", "v2"]), "V3");
});
