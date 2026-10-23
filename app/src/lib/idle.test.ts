import { test } from "node:test";
import assert from "node:assert/strict";
import type { IdleScreen } from "./api/index.ts";
import { describeIdle, effectiveIdle, idleKind, stepSlide } from "./idle.ts";

const picture: IdleScreen = { Image: { blob: "a", name: "Welcome.png" } };
const video: IdleScreen = { Video: { blob: "b", name: "Loop.mp4" } };
const slide: IdleScreen = { DeckSlide: { deck: "d1", slide: 2 } };
const decks = [{ id: "d1", title: "Announcements" }];

test("each setting is one kind", () => {
  assert.equal(idleKind(null), "Inherit");
  assert.equal(idleKind("Blank"), "Blank");
  assert.equal(idleKind(picture), "Image");
  assert.equal(idleKind(video), "Video");
  assert.equal(idleKind(slide), "DeckSlide");
});

test("settings are described in plain words", () => {
  assert.equal(describeIdle(null, decks), "Same as the other screens");
  assert.equal(describeIdle("Blank", decks), "The theme's plain background");
  assert.equal(describeIdle(picture, decks), "Picture: Welcome.png");
  assert.equal(describeIdle(video, decks), "Looping video: Loop.mp4");
  assert.equal(describeIdle(slide, decks), "Slide 3 of Announcements");
  assert.match(describeIdle({ DeckSlide: { deck: "gone", slide: 0 } }, decks), /no longer/);
});

test("a screen's own setting beats the default, and plain means nothing", () => {
  assert.equal(effectiveIdle(picture, video), picture);
  assert.equal(effectiveIdle(null, video), video);
  assert.equal(effectiveIdle("Blank", video), null);
  assert.equal(effectiveIdle(null, null), null);
  assert.equal(effectiveIdle(null, "Blank"), null);
});

test("stepping through a deck stops at both ends", () => {
  assert.equal(stepSlide(0, -1, 5), 0);
  assert.equal(stepSlide(3, 1, 5), 4);
  assert.equal(stepSlide(4, 1, 5), 4);
  assert.equal(stepSlide(0, 1, 0), 0);
});
