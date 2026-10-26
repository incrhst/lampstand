// node --test --experimental-strip-types (npm test)
import { test } from "node:test";
import assert from "node:assert/strict";
import type { TrainingVideos } from "./api/index.ts";
import { trainingVideosBusy, trainingVideosLine } from "./trainingVideos.ts";

const offline = "The training videos download the first time. Connect to the internet to watch them.";

test("says nothing when the training videos are here, or before the core has said", () => {
  assert.equal(trainingVideosLine(null), null);
  assert.equal(trainingVideosLine({ state: "Ready", message: "" }), null);
});

test("shows how far the download has got", () => {
  const videos: TrainingVideos = { state: { Downloading: { done: 2_500_000, total: 10_000_000 } }, message: "" };
  assert.equal(trainingVideosLine(videos), "Downloading the training videos: 25%");
  assert.equal(trainingVideosBusy(videos), true);
  assert.equal(
    trainingVideosLine({ state: { Downloading: { done: 0, total: 0 } }, message: "" }),
    "Downloading the training videos…",
  );
});

test("gives the core's plain message offline, and before the first try", () => {
  const videos: TrainingVideos = { state: "Offline", message: offline };
  assert.equal(trainingVideosLine(videos), offline);
  assert.equal(trainingVideosBusy(videos), false);
  const first = "The training videos download the first time you open a training service.";
  assert.equal(trainingVideosLine({ state: "NotDownloaded", message: first }), first);
});
