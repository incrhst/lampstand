import { test } from "node:test";
import assert from "node:assert/strict";
import type { Service, Sound, SoundFileStatus } from "./api.ts";
import {
  audioExtensions,
  hasSound,
  isAudio,
  isMissing,
  lengthLabel,
  missingFiles,
  soundInUse,
  soundSubtitle,
  soundTotalLabel,
  startLabel,
  words,
} from "./sound.ts";

const sound = (over: Partial<Sound> = {}): Sound => ({
  name: "Walk-in music",
  file: "a".repeat(64),
  length_ms: 185_000,
  start: "Waits",
  keep_off_stream: false,
  ...over,
});

const service = (items: Service["items"]): Service => ({
  id: "s",
  title: "Sunday",
  starts_at: null,
  time_zone: null,
  theme: null,
  items,
});

const header = { position: "a", kind: { Header: { title: "Worship" } }, notes: "", obs_scene: null, loop_seconds: null };
const musicItem = (over: Partial<Sound> = {}) => ({
  position: "b",
  kind: { Sound: { sound: sound(over) } },
  notes: "",
  obs_scene: null,
  loop_seconds: null,
});

test("sound files are told by their extension, in any case", () => {
  assert.equal(isAudio("Walk-in.MP3"), true);
  assert.equal(isAudio("track.flac"), true);
  assert.equal(isAudio("intro.mp4"), false);
  assert.equal(isAudio("no extension"), false);
  assert.ok(audioExtensions.every((e) => isAudio(`x.${e}`)));
});

test("a sound's second line says what it is, how long and how it starts", () => {
  assert.equal(soundSubtitle(sound()), "Music only · 3:05 · Waits for someone to press play");
  assert.equal(soundSubtitle(sound({ length_ms: null, start: "OnItsOwn" })), "Music only · length unknown · Plays on its own");
  assert.equal(lengthLabel(61_000), "1:01");
  assert.equal(startLabel("OnItsOwn"), words.onItsOwn);
});

test("a service with no sound shows no sound total and is not 'in use'", () => {
  const plain = service({ h: header });
  assert.equal(hasSound(plain), false);
  assert.equal(soundTotalLabel(plain), null);
  // No localStorage under node: nothing remembered.
  assert.equal(soundInUse(plain), false);
  assert.equal(soundInUse(null), false);
});

test("music-only items add up in the service's sound total", () => {
  const s = service({ h: header, a: musicItem(), b: musicItem({ length_ms: 60_000 }) });
  assert.equal(hasSound(s), true);
  assert.equal(soundInUse(s), true);
  assert.equal(soundTotalLabel(s), "Sound: 4:05");
  const unknown = service({ a: musicItem(), b: musicItem({ length_ms: null }) });
  assert.equal(soundTotalLabel(unknown), "Sound: at least 3:05");
});

test("a backing track counts as sound but adds no time", () => {
  const song = {
    position: "a",
    kind: { Song: { song: "x", arrangement: null } },
    notes: "",
    obs_scene: null,
    loop_seconds: null,
    backing: sound({ name: "Track" }),
  };
  const s = service({ song });
  assert.equal(hasSound(s), true);
  assert.equal(soundTotalLabel(s), null);
});

test("the files that are not here are named by item", () => {
  const status: SoundFileStatus[] = [
    { item: "a", backing: false, file: "1", on_device: true },
    { item: "b", backing: false, file: "2", on_device: false },
    { item: "c", backing: true, file: "3", on_device: false },
  ];
  assert.deepEqual([...missingFiles(status)].sort(), ["b", "c"]);
  assert.equal(isMissing(status, "b", false), true);
  assert.equal(isMissing(status, "c", false), false);
  assert.equal(isMissing(status, "c", true), true);
  assert.equal(isMissing(status, "a", false), false);
});
