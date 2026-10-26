import { test } from "node:test";
import assert from "node:assert/strict";
import type { LoadedService, SlideView } from "./api";
import type { LiveState } from "./remote.ts";
import { clock, defaultSpeakerItem, elapsed, liveSince, speakerChoices, speakerView } from "./speaker.ts";

const text = (label: string, notes = ""): SlideView => ({ label, lines: ["line"], footer: null, steps: 0, notes });
const deck = (n: number, steps = 0, notes = ""): SlideView => ({ label: `Slide ${n}`, lines: [], footer: null, steps, notes });

const loaded: LoadedService = {
  service: "s1",
  title: "Sunday",
  sections: [],
  items: [
    { id: "intro", title: "Notices", slides: [deck(1)], sounds: [] },
    { id: "song", title: "Amazing Grace", slides: [text("V1"), text("V2")], sounds: [] },
    { id: "talk", title: "Sermon", slides: [deck(1, 0, "Welcome"), deck(2, 2, "Three points"), deck(3)], sounds: [] },
    { id: "music", title: "Walk-out", slides: [], sounds: [] },
    { id: "verse", title: "John 3:16", slides: [text("John 3:16")], sounds: [] },
  ],
};

const at = (item: string, slide: number, step = 0, overlay: LiveState["overlay"] = "None"): LiveState => ({
  live: { item, slide, step },
  preview: null,
  idle: false,
  overlay,
});

test("the dialog offers items with slides, the first deck after the live item first", () => {
  assert.deepEqual(speakerChoices(loaded).map((i) => i.id), ["intro", "song", "talk", "verse"]);
  assert.equal(defaultSpeakerItem(loaded, "song"), "talk");
  assert.equal(defaultSpeakerItem(loaded, "talk"), "intro", "no deck after the talk: the first deck");
  assert.equal(defaultSpeakerItem(loaded, null), "intro");
  const lyrics: LoadedService = { ...loaded, items: [loaded.items[1], loaded.items[4]] };
  assert.equal(defaultSpeakerItem(lyrics, null), "song", "no deck at all: the first item with slides");
  assert.equal(defaultSpeakerItem(null, null), null);
});

test("the page waits while another item is on the screen, or the idle screen", () => {
  assert.deepEqual(speakerView(loaded, at("song", 1), "talk"), { state: "waiting", title: "Sermon", onScreen: "Amazing Grace · V2" });
  assert.deepEqual(speakerView(loaded, null, "talk"), { state: "waiting", title: "Sermon", onScreen: null });
  assert.deepEqual(speakerView(loaded, at("music", 0), "talk"), { state: "waiting", title: "Sermon", onScreen: "Walk-out" });
  assert.equal(speakerView(loaded, at("talk", 0, 0, "Idle"), "talk").state, "waiting");
  assert.deepEqual(speakerView(loaded, at("talk", 0), "gone"), { state: "gone" });
});

test("live: the slide, its place, the next click and the notes", () => {
  const first = speakerView(loaded, at("talk", 0), "talk");
  assert.equal(first.state, "live");
  if (first.state !== "live") return;
  assert.equal(first.position, "Slide 1 of 3");
  assert.equal(first.first, true);
  assert.equal(first.last, false);
  assert.equal(first.notes, "Welcome");
  assert.deepEqual(first.next && { kind: first.next.kind, cursor: first.next.cursor }, { kind: "slide", cursor: { item: "talk", slide: 1, step: 0 } });

  const built = speakerView(loaded, at("talk", 1, 1, "Black"), "talk");
  if (built.state !== "live") return assert.fail();
  assert.equal(built.position, "Slide 2 of 3 · build 1 of 2");
  assert.equal(built.blank, true);
  assert.equal(built.first, false);
  assert.equal(built.next?.kind, "click");
  if (built.next?.kind === "click") {
    assert.equal(built.next.clicksLeft, 1);
    assert.deepEqual(built.next.cursor, { item: "talk", slide: 1, step: 2 });
  }

  const full = speakerView(loaded, at("talk", 1, 2), "talk");
  if (full.state !== "live") return assert.fail();
  assert.equal(full.position, "Slide 2 of 3 · build 2 of 2");
  assert.equal(full.next?.kind, "slide");
  if (full.next?.kind === "slide") assert.equal(full.next.title, "Slide 3");
});

test("a cleared screen stops the speaker until the slides are back", () => {
  const cleared = { ...at("talk", 1), cleared: true };
  assert.deepEqual(speakerView(loaded, cleared, "talk"), { state: "cleared", title: "Sermon" });
  assert.equal(speakerView(loaded, { ...cleared, cleared: false }, "talk").state, "live");
  // An older laptop says nothing about it: live, as before.
  assert.equal(speakerView(loaded, at("talk", 1), "talk").state, "live");
  // Another item cleared: still just waiting.
  assert.equal(speakerView(loaded, { ...at("song", 0), cleared: true }, "talk").state, "waiting");
});

test("the last slide fully built greys Next", () => {
  const last = speakerView(loaded, at("talk", 2), "talk");
  if (last.state !== "live") return assert.fail();
  assert.equal(last.last, true);
  assert.equal(last.next, null);
});

test("lyrics and scripture work too: text slides, no builds", () => {
  const v = speakerView(loaded, at("song", 0), "song");
  if (v.state !== "live") return assert.fail();
  assert.equal(v.position, "Slide 1 of 2");
  assert.equal(v.next?.kind === "slide" && v.next.title, "V2");
  const one = speakerView(loaded, at("verse", 0), "verse");
  if (one.state !== "live") return assert.fail();
  assert.equal(one.first && one.last, true, "a single slide is both");
});

test("the timer starts when the item goes live and starts again the next time", () => {
  let since = liveSince(null, false, 1000);
  assert.equal(since, null);
  since = liveSince(since, true, 2000);
  assert.equal(since, 2000);
  since = liveSince(since, true, 9000);
  assert.equal(since, 2000, "kept while on air");
  since = liveSince(since, false, 10_000);
  since = liveSince(since, true, 20_000);
  assert.equal(since, 20_000, "reset when it goes live again");
  assert.equal(elapsed(245_000), "4:05");
  assert.equal(elapsed(3_729_000), "1:02:09");
  assert.equal(elapsed(-5), "0:00");
  assert.equal(clock(new Date(2026, 9, 25, 9, 5)), "9:05");
});
