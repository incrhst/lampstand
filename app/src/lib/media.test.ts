import { test } from "node:test";
import assert from "node:assert/strict";
import { deckSourceLabel, isVideo, mediaKind, onePath } from "./media.ts";

test("video is told from images by extension, in any case", () => {
  assert.equal(isVideo("Welcome loop.MP4"), true);
  assert.equal(isVideo("countdown.mov"), true);
  assert.equal(isVideo("announcements.png"), false);
  assert.equal(isVideo("no extension"), false);
  assert.equal(mediaKind("intro.webm"), "Video");
  assert.equal(mediaKind("slide.jpg"), "Image");
});

test("a deck's source reads as where it came from", () => {
  assert.equal(deckSourceLabel({ File: { name: "Easter.pptx" } }), "From a file");
  assert.equal(deckSourceLabel({ GoogleSlidesLink: { url: "https://docs.google.com/x" } }), "Google Slides link");
  assert.equal(deckSourceLabel({ OneDriveLink: { url: "https://1drv.ms/x" } }), "OneDrive link");
  assert.equal(deckSourceLabel({ GuestUpload: { uploader: "Pastor Ann" } }), "Uploaded by Pastor Ann");
});

test("a picker's answer becomes one path or none", () => {
  assert.equal(onePath(null), null);
  assert.equal(onePath("/a.pptx"), "/a.pptx");
  assert.equal(onePath(["/a.pptx", "/b.pptx"]), "/a.pptx");
  assert.equal(onePath([]), null);
});
