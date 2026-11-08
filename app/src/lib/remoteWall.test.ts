import { test } from "node:test";
import assert from "node:assert/strict";
import type { LoadedService, SlideView } from "./api";
import type { LiveState } from "./remote.ts";
import {
  PictureQueue,
  keepSelection,
  layoutFor,
  liveKey,
  nextLabel,
  selectionLine,
  serviceShape,
  status,
  tap,
  twoStepTap,
  wallGroups,
} from "./remoteWall.ts";

const slide = (label: string | null, lines: string[] = ["line"], steps = 0): SlideView => ({ label, lines, footer: null, steps, notes: "" });

const loaded: LoadedService = {
  service: "s1",
  title: "Sunday",
  sections: [{ header: "h-worship", title: "Worship", seconds: null, items: ["song", "deck"] }],
  items: [
    { id: "walk", title: "Walk-in", slides: [], sounds: [{ name: "Walk-in music", backing: false, length_ms: 185_000, start: "Waits" }] },
    { id: "h-worship", title: "Worship", slides: [], sounds: [] },
    { id: "song", title: "Amazing Grace", slides: [slide("V1"), slide("V2")], sounds: [{ name: "Grace track", backing: true, length_ms: null, start: "OnItsOwn" }] },
    { id: "deck", title: "Notices", slides: [slide("Slide 1", [], 2), slide(null, [])], sounds: [] },
    { id: "out", title: "Walk-out", slides: [], sounds: [{ name: "Walk-out music", backing: false, length_ms: null, start: "OnItsOwn" }] },
    { id: "empty", title: "Prayer", slides: [], sounds: [] },
  ],
};

const at = (item: string, s: number, step = 0, more: Partial<LiveState> = {}): LiveState => ({
  live: { item, slide: s, step },
  preview: null,
  idle: false,
  overlay: "None",
  ...more,
});

test("the wall groups rows under their headings and numbers them as Present does", () => {
  const groups = wallGroups(loaded, null);
  assert.deepEqual(
    groups.map((g) => [g.heading?.title ?? null, g.rows.map((r) => [r.number, r.item.title, r.subtitle])]),
    [
      [null, [[1, "Walk-in", "Music only"]]],
      [
        "Worship",
        [
          [3, "Amazing Grace", "2 slides"],
          [4, "Notices", "2 slides"],
          [5, "Walk-out", "Music only"],
          [6, "Prayer", "Nothing to show"],
        ],
      ],
    ],
  );
  assert.deepEqual(wallGroups(null, null), []);
});

test("music-only items get a sound card that says how they start and how long they are", () => {
  const rows = wallGroups(loaded, null).flatMap((g) => g.rows);
  assert.deepEqual(rows[0].sound, { name: "Walk-in music", start: "Waits for Play", length: "3:05" });
  assert.deepEqual(rows[3].sound, { name: "Walk-out music", start: "Plays on its own", length: "length unknown" });
  assert.equal(rows[1].sound, null, "a backing track is not a music-only item");
  assert.equal(rows[4].sound, null);
});

test("the slide on air is LIVE, the one Next shows is NEXT, and earlier items are played", () => {
  const rows = wallGroups(loaded, at("song", 1)).flatMap((g) => g.rows);
  const song = rows[1];
  assert.equal(song.onAir, true);
  assert.equal(song.subtitle, "On air");
  assert.deepEqual(song.cards.map((c) => c.mark), [null, "live"]);
  assert.deepEqual(rows[2].cards.map((c) => c.mark), ["next", null]);
  assert.deepEqual(rows.map((r) => r.played), [true, false, false, false, false]);
  // Deck slides want pictures; a slide with no label is called by its number.
  assert.deepEqual(rows[2].cards.map((c) => [c.label, c.picture]), [["Slide 1", true], ["2", true]]);
});

test("with nothing on air, the first slide of the service is NEXT and nothing is played", () => {
  const rows = wallGroups(loaded, { live: null, preview: null, idle: true, overlay: "None" }).flatMap((g) => g.rows);
  assert.equal(rows[1].cards[0].mark, "next");
  assert.ok(rows.every((r) => !r.played && !r.onAir));
});

test("a music-only item on air is the row on air", () => {
  const rows = wallGroups(loaded, at("out", 0, 0, { shown: { item: "deck", slide: 1, step: 0 } })).flatMap((g) => g.rows);
  assert.equal(rows[3].onAir, true);
  assert.equal(rows[2].played, true);
});

test("the top strip says LIVE, IDLE or BLANK with the item and slide", () => {
  assert.deepEqual(status(loaded, at("song", 0)), { tone: "live", label: "LIVE", item: "Amazing Grace", detail: "V1" });
  assert.deepEqual(status(loaded, at("song", 1, 0, { overlay: "Black" })), { tone: "blank", label: "BLANK", item: "Amazing Grace", detail: "The screen is blank" });
  assert.deepEqual(status(loaded, at("song", 1, 0, { overlay: "Idle" })), { tone: "idle", label: "IDLE", item: null, detail: "Show returns to Amazing Grace · V2" });
  assert.equal(status(loaded, { live: null, preview: null, idle: true, overlay: "None" }).detail, "Nothing on air yet. Next shows the first slide.");
  assert.equal(status(null, null).tone, "idle");
});

test("during builds the strip says which build, and Next how many are left", () => {
  assert.equal(status(loaded, at("deck", 0, 0)).detail, "Slide 1");
  assert.equal(status(loaded, at("deck", 0, 1)).detail, "Slide 1 · build 1 of 2");
  assert.equal(nextLabel(loaded, at("deck", 0, 0)), "Next · 2 left");
  assert.equal(nextLabel(loaded, at("deck", 0, 1)), "Next · 1 left");
  assert.equal(nextLabel(loaded, at("deck", 0, 2)), "Next", "fully built");
  assert.equal(nextLabel(loaded, at("song", 0)), "Next");
  assert.equal(nextLabel(loaded, null), "Next");
});

test("while music plays the strip shows the slide still on the screens and what plays", () => {
  const s = status(loaded, at("out", 0, 0, { shown: { item: "deck", slide: 1, step: 0 } }));
  assert.deepEqual(s, { tone: "live", label: "LIVE", item: "Notices", detail: "2 · Walk-out" });
});

test("a first tap selects, a second tap on the same card shows it", () => {
  const v1 = { item: "song", slide: 0, step: 0 };
  const v2 = { item: "song", slide: 1, step: 0 };
  let r = tap(null, v1);
  assert.deepEqual(r, { selected: v1, show: null });
  r = tap(r.selected, v2);
  assert.deepEqual(r, { selected: v2, show: null }, "another card moves the selection");
  r = tap(r.selected, v2);
  assert.deepEqual(r, { selected: null, show: v2 });
  assert.equal(selectionLine(loaded, v2), "Amazing Grace · V2");
  assert.equal(selectionLine(loaded, null), "");
});

test("any live change cancels the selection, and a reload drops one that is gone", () => {
  const base = at("song", 0);
  assert.equal(liveKey(base), liveKey(at("song", 0)));
  assert.notEqual(liveKey(base), liveKey(at("song", 1)));
  assert.notEqual(liveKey(base), liveKey(at("song", 0, 0, { overlay: "Black" })));
  assert.notEqual(liveKey(base), liveKey(at("song", 0, 0, { idle: true })));
  assert.notEqual(liveKey(base), liveKey(null), "the link dropping");
  assert.deepEqual(keepSelection(loaded, { item: "song", slide: 1, step: 0 }), { item: "song", slide: 1, step: 0 });
  assert.equal(keepSelection(loaded, { item: "song", slide: 5, step: 0 }), null);
  assert.equal(keepSelection(null, { item: "song", slide: 0, step: 0 }), null);
});

test("700 px and up is the wide wall; touch keeps the two-step tap", () => {
  assert.equal(layoutFor(390), "narrow");
  assert.equal(layoutFor(699), "narrow");
  assert.equal(layoutFor(700), "wide");
  assert.equal(layoutFor(1024), "wide");
  assert.equal(twoStepTap(true, "wide"), true, "a tablet");
  assert.equal(twoStepTap(false, "narrow"), true, "a narrow window");
  assert.equal(twoStepTap(false, "wide"), false, "a mouse on a laptop");
});

test("the same service said again keeps its shape; a changed deck does not", () => {
  assert.equal(serviceShape(loaded), serviceShape(structuredClone(loaded)));
  const changed = structuredClone(loaded);
  changed.items[3].slides.push(slide("Slide 3", []));
  assert.notEqual(serviceShape(loaded), serviceShape(changed));
  assert.equal(serviceShape(null), "");
});

/** A fetcher whose answers the test gives by hand. */
function manual() {
  const calls: { job: string; answer: (p: string | null) => void; fail: () => void }[] = [];
  const fetch = (job: string) =>
    new Promise<string | null>((resolve, reject) => calls.push({ job, answer: resolve, fail: () => reject(new Error("no")) }));
  return { calls, fetch };
}
const settle = () => new Promise((r) => setTimeout(r, 0));

test("pictures are asked for two at a time, most urgent first, and kept", async () => {
  const { calls, fetch } = manual();
  const got: string[] = [];
  const q = new PictureQueue<string>(fetch, (key) => got.push(key), 2);
  for (const k of ["a", "b", "c", "d"]) q.want(k, k, 5);
  q.want("live", "live", 0);
  assert.deepEqual(calls.map((c) => c.job), ["a", "b"]);
  assert.equal(q.busy, 2);
  calls[0].answer("pic-a");
  await settle();
  assert.deepEqual(calls.map((c) => c.job), ["a", "b", "live"], "the slide on air jumps the queue");
  assert.equal(q.get("a"), "pic-a");
  q.want("a", "a", 0);
  await settle();
  assert.equal(calls.length, 3, "a kept picture is not asked for again");
  q.drop("d");
  calls[1].answer("pic-b");
  calls[2].answer("pic-live");
  await settle();
  assert.deepEqual(calls.map((c) => c.job), ["a", "b", "live", "c"], "d left the screen before its turn");
  assert.deepEqual(got, ["a", "b", "live"]);
});

test("a picture the laptop cannot give waits for a retry; a reset drops old answers", async () => {
  const { calls, fetch } = manual();
  const got: string[] = [];
  const q = new PictureQueue<string>(fetch, (key) => got.push(key), 2);
  q.want("a", "a", 1);
  calls[0].fail();
  await settle();
  q.want("a", "a", 1);
  assert.equal(calls.length, 1, "not asked again yet");
  q.retry();
  q.want("a", "a", 1);
  assert.equal(calls.length, 2);
  q.reset();
  calls[1].answer("old");
  await settle();
  assert.equal(q.get("a"), undefined);
  assert.deepEqual(got, []);
  q.want("a", "a", 1);
  assert.equal(calls.length, 3, "asked afresh after the reset");
});
