import { test } from "node:test";
import assert from "node:assert/strict";
import type { LoadedService, SlideView } from "./api";
import { backoff, guessName, onScreen, pinDigits, pictureKey, remoteSounds, slideLine, upNext, wantsPicture, type LiveState } from "./remote.ts";

const slide = (label: string, lines: string[] = ["line"], steps = 0): SlideView => ({ label, lines, footer: null, steps, notes: "" });

const loaded: LoadedService = {
  service: "s1",
  title: "Sunday",
  sections: [],
  items: [
    { id: "walk", title: "Walk-in", slides: [], sounds: [{ name: "Walk-in music", backing: false, length_ms: 185_000, start: "Waits" }] },
    { id: "song", title: "Amazing Grace", slides: [slide("V1"), slide("V2", ["x"], 0)], sounds: [{ name: "Grace track", backing: true, length_ms: null, start: "OnItsOwn" }] },
    { id: "deck", title: "Notices", slides: [slide("Slide 1", [], 2)], sounds: [] },
  ],
};

const at = (item: string, s: number, step = 0): LiveState => ({ live: { item, slide: s, step }, preview: null, idle: false, overlay: "None" });

test("backoff doubles from half a second and stops at ten", () => {
  assert.equal(backoff(0, 0), 500);
  assert.equal(backoff(1, 0), 1000);
  assert.equal(backoff(3, 0), 4000);
  assert.equal(backoff(30, 0), 10_000);
  assert.equal(backoff(30, 1), 12_000);
});

test("a phone's name is guessed from its browser", () => {
  assert.equal(guessName("Mozilla/5.0 (iPhone; CPU iPhone OS 16_0 like Mac OS X) Safari/604.1"), "iPhone");
  assert.equal(guessName("Mozilla/5.0 (Linux; Android 14; Pixel 8) Chrome/120 Mobile Safari/537.36"), "Android phone");
  assert.equal(guessName("Mozilla/5.0 (Linux; Android 13; SM-X200) Chrome/120 Safari/537.36"), "Android tablet");
  assert.equal(guessName("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) Version/17.0 Safari/605.1.15"), "iPad");
  assert.equal(guessName("Mozilla/5.0 (Windows NT 10.0) Chrome/120"), "Phone");
});

test("a PIN is its six digits however it was typed", () => {
  assert.equal(pinDigits("482 913"), "482913");
  assert.equal(pinDigits("482-9131"), "482913");
});

test("on screen is the live cursor's slide, and nothing when nothing is live", () => {
  assert.equal(onScreen(loaded, null), null);
  assert.equal(onScreen(null, at("song", 0)), null);
  assert.equal(onScreen(loaded, at("song", 1))?.slide.label, "V2");
  assert.equal(onScreen(loaded, at("gone", 0)), null);
});

test("up next follows Next: build steps, then slides, skipping items with none", () => {
  assert.equal(upNext(loaded, null)?.title, "Amazing Grace");
  assert.equal(upNext(loaded, at("song", 0))?.slide.label, "V2");
  assert.equal(upNext(loaded, at("song", 1))?.title, "Notices");
  assert.equal(upNext(loaded, at("deck", 0))?.build, true);
  assert.equal(upNext(loaded, at("deck", 0, 2)), null);
});

test("slides without text ask for a picture", () => {
  assert.equal(wantsPicture(slide("Slide 1", [])), true);
  assert.equal(wantsPicture(slide("V1")), false);
  assert.equal(slideLine("Amazing Grace", slide("V1")), "Amazing Grace · V1");
  assert.equal(slideLine("Notices", { ...slide("", []), label: null }), "Notices");
  assert.notEqual(pictureKey(loaded, at("deck", 0).live!), pictureKey(loaded, at("deck", 0, 1).live!));
});

test("the sound box lists music-only items and backing tracks in order", () => {
  const sounds = remoteSounds(loaded);
  assert.deepEqual(
    sounds.map((s) => [s.item, s.backing, s.name]),
    [
      ["walk", false, "Walk-in music"],
      ["song", true, "Grace track"],
    ],
  );
  assert.equal(sounds[0].detail, "3:05");
  assert.equal(sounds[1].detail, "Backing track · Amazing Grace · length unknown");
  assert.deepEqual(remoteSounds(null), []);
});
