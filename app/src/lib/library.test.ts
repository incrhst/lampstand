import { test } from "node:test";
import assert from "node:assert/strict";
import {
  byFilter,
  countKinds,
  importedCounts,
  isLink,
  linkEntries,
  mightBeReference,
  newSongs,
  rank,
  remember,
  songProgress,
  sortPaths,
  summarize,
  titleScore,
  type Entry,
  type Kind,
} from "./library.ts";

const entry = (kind: Kind, title: string): Entry => ({ key: `${kind}:${title}`, kind, title, subtitle: "", item: null });

test("titles score by how they match", () => {
  assert.equal(titleScore("Amazing Grace", "amaz"), 3);
  assert.equal(titleScore("Amazing Grace", "grace"), 2);
  assert.equal(titleScore("Amazing Grace", "grace amazing"), 1);
  assert.equal(titleScore("Amazing Grace", "holy"), 0);
  assert.equal(titleScore("Amazing Grace", "  "), 0);
});

test("a search ranks title hits above lyric hits, across kinds", () => {
  const songs = [entry("song", "Grace Alone"), entry("song", "How Great Thou Art")]; // the second is a lyric hit
  const others = [entry("deck", "Sermon: grace"), entry("media", "Welcome loop.mp4"), entry("deck", "Grace notes")];
  const titles = rank("grace", null, songs, others).map((e) => e.title);
  assert.deepEqual(titles, ["Grace Alone", "Grace notes", "Sermon: grace", "How Great Thou Art"]);
});

test("a reference goes first", () => {
  const passage = entry("scripture", "Psalms 23");
  const ranked = rank("ps 23", passage, [entry("song", "Psalm 23 (I'm Not Alone)")], []);
  assert.equal(ranked[0], passage);
  assert.equal(ranked.length, 2);
});

test("songs that tie keep the core's order", () => {
  const songs = [entry("song", "Zion"), entry("song", "Abide")];
  assert.deepEqual(rank("x", null, songs, []).map((e) => e.title), ["Zion", "Abide"]);
});

test("counts and filters by kind", () => {
  const list = [entry("song", "a"), entry("song", "b"), entry("deck", "c"), entry("scripture", "d")];
  assert.deepEqual(countKinds(list), { all: 4, song: 2, scripture: 1, deck: 1, media: 0 });
  assert.deepEqual(byFilter(list, "deck").map((e) => e.title), ["c"]);
  assert.equal(byFilter(list, "all").length, 4);
});

test("recently used keeps each once, newest first", () => {
  let recent: Entry[] = [];
  for (const t of ["a", "b", "c", "a"]) recent = remember(recent, entry("song", t), 3);
  assert.deepEqual(recent.map((e) => e.title), ["a", "c", "b"]);
  recent = remember(recent, entry("deck", "d"), 3);
  assert.deepEqual(recent.map((e) => e.title), ["d", "a", "c"]);
});

test("dropped paths go to the request that takes them", () => {
  const sorted = sortPaths([
    "/Users/me/Songs",
    "/Users/me/Downloads/English.osb",
    "C:\\Talks\\Sermon.PPTX",
    "/Users/me/Welcome loop.mp4",
    "/Users/me/photo.JPG",
    "/Users/me/kjv.xml",
    "/Users/me/.DS_Store",
    "/Users/me/Amazing Grace",
    "/Users/me/Walk-in.MP3",
    "/Users/me/track.m4a",
    "/Users/me/bell.wav",
  ]);
  assert.deepEqual(sorted, {
    imports: ["/Users/me/Songs", "/Users/me/Downloads/English.osb", "/Users/me/kjv.xml", "/Users/me/Amazing Grace"],
    decks: ["C:\\Talks\\Sermon.PPTX"],
    media: ["/Users/me/Welcome loop.mp4", "/Users/me/photo.JPG"],
    sounds: ["/Users/me/Walk-in.MP3", "/Users/me/track.m4a", "/Users/me/bell.wav"],
  });
});

test("reads the import summary line", () => {
  assert.deepEqual(importedCounts("Imported 3 songs. 2 already in the library"), { songs: 3, bibles: 0, updated: 0 });
  assert.deepEqual(importedCounts("Imported 1 song. Bibles: King James Version, WEB"), { songs: 1, bibles: 2, updated: 0 });
  assert.deepEqual(importedCounts("Imported 0 songs. skipped 1: photo.png: images"), { songs: 0, bibles: 0, updated: 0 });
  assert.deepEqual(importedCounts("Imported 0 songs. 1 updated. 1 already in the library"), { songs: 0, bibles: 0, updated: 1 });
});

test("summarises what was added", () => {
  assert.equal(summarize({ songs: 3, decks: 1 }), "3 songs · 1 deck");
  assert.equal(summarize({ songs: 1, bibles: 1, media: 2 }), "1 song · 1 Bible · 2 media files");
  assert.equal(summarize({ sounds: 2 }), "2 sounds");
  assert.equal(summarize({ updated: 1 }), "1 song updated");
  assert.equal(summarize({}), "Nothing new to add");
});

test("reads song import progress", () => {
  assert.equal(songProgress("Adding songs: 300 of 1200"), 0.25);
  assert.equal(songProgress("Downloading the en song database"), null);
});

test("links and references", () => {
  assert.ok(isLink("https://docs.google.com/presentation/d/abc/edit"));
  assert.ok(!isLink("https grace"));
  assert.ok(mightBeReference("Rom 8:28"));
  assert.ok(!mightBeReference("Jude"));
});

test("recent holds what was just added as well as what was used, and using a new row clears its mark", () => {
  let recent: Entry[] = [];
  recent = remember(recent, { ...entry("deck", "Easter deck"), fresh: true });
  recent = remember(recent, entry("song", "Amazing Grace"));
  assert.deepEqual(recent.map((e) => [e.title, !!e.fresh]), [["Amazing Grace", false], ["Easter deck", true]]);
  // Putting the new deck in a service moves it up and drops the "New" mark.
  recent = remember(recent, { ...entry("deck", "Easter deck"), fresh: false });
  assert.deepEqual(recent.map((e) => [e.title, !!e.fresh]), [["Easter deck", false], ["Amazing Grace", false]]);
});

test("a few new songs are listed as recent; a whole songbook is not", () => {
  const hit = (id: string) => ({ song: id, title: id, line: null });
  const before = ["a", "b"];
  assert.deepEqual(newSongs(before, [hit("a"), hit("b"), hit("c")]).map((h) => h.song), ["c"]);
  assert.deepEqual(newSongs(before, [hit("a"), hit("b")]), []);
  const book = Array.from({ length: 40 }, (_, i) => hit(`s${i}`));
  assert.deepEqual(newSongs(before, [...book, hit("a")]), [], "too many to be recent");
  assert.equal(newSongs(before, book, 50).length, 40, "unless the limit allows it");
});

test("a pasted link offers the rows it could be added as", () => {
  const keys = (url: string) => linkEntries(url).map((e) => e.key);
  assert.deepEqual(keys("https://docs.google.com/presentation/d/abc/edit"), ["link"]);
  assert.deepEqual(keys("https://drive.google.com/file/d/abc/view"), ["link", "link:sound"]);
  assert.deepEqual(keys("https://www.dropbox.com/s/abc/Walk-in.mp3?dl=0"), ["link:sound"]);
  const [refused] = linkEntries("https://open.spotify.com/track/abc");
  assert.equal(refused.key, "link:streaming");
  assert.equal(refused.item, null);
  assert.match(refused.subtitle, /Streaming links can't be added/);
});
