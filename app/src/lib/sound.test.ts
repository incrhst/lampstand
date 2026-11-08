import { test } from "node:test";
import assert from "node:assert/strict";
import type { Service, Sound, SoundFileStatus } from "./api.ts";
import {
  audioExtensions,
  hasSound,
  isAudio,
  isComing,
  linkOffers,
  isMissing,
  lengthLabel,
  missingFiles,
  offerSound,
  soundSubtitle,
  soundTotalLabel,
  streamingRefusal,
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
  // Off: nothing is offered for a service without sound, or for no service.
  assert.equal(offerSound(plain, false), false);
  assert.equal(offerSound(plain, undefined), false);
  assert.equal(offerSound(null, false), false);
});

test("music-only items add up in the service's sound total", () => {
  const s = service({ h: header, a: musicItem(), b: musicItem({ length_ms: 60_000 }) });
  assert.equal(hasSound(s), true);
  // A service with sound offers it even with the switch off.
  assert.equal(offerSound(s, false), true);
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
    { item: "a", backing: false, file: "1", on_device: true, coming: false },
    { item: "b", backing: false, file: "2", on_device: false, coming: true },
    { item: "c", backing: true, file: "3", on_device: false, coming: false },
  ];
  assert.deepEqual([...missingFiles(status)].sort(), ["b", "c"]);
  assert.equal(isMissing(status, "b", false), true);
  assert.equal(isMissing(status, "c", false), false);
  assert.equal(isMissing(status, "c", true), true);
  assert.equal(isMissing(status, "a", false), false);
  // Only a missing file that is on its way is "coming".
  assert.equal(isComing(status, "b", false), true);
  assert.equal(isComing(status, "c", true), false);
  assert.equal(isComing(status, "a", false), false);
});

test("the Setup switch turns the offer on for any service", () => {
  const plain = service({ h: header });
  assert.equal(offerSound(plain, true), true);
  assert.equal(offerSound(null, true), true);
});

test("a pasted link is read as a deck, a sound, or a streaming page to refuse", () => {
  const sound = { deck: false, sound: true, streaming: false };
  const deck = { deck: true, sound: false, streaming: false };
  const either = { deck: true, sound: true, streaming: false };
  const streaming = { deck: false, sound: false, streaming: true };
  assert.deepEqual(linkOffers("https://drive.google.com/file/d/abc/view"), either);
  assert.deepEqual(linkOffers("https://1drv.ms/u/s!AbC"), either);
  assert.deepEqual(linkOffers("https://www.dropbox.com/scl/fi/x/Walk%20in.mp3?dl=0"), sound);
  assert.deepEqual(linkOffers("example.org/music/Be-Thou.FLAC"), sound);
  assert.deepEqual(linkOffers("https://docs.google.com/presentation/d/abc/edit"), deck);
  assert.deepEqual(linkOffers("https://example.org/deck.pptx"), deck);
  for (const url of [
    "https://www.youtube.com/watch?v=abc",
    "https://youtu.be/abc",
    "https://music.youtube.com/watch?v=abc",
    "https://open.spotify.com/track/abc",
    "https://music.apple.com/us/album/x/1",
    "https://soundcloud.com/a/b.mp3",
  ]) {
    assert.deepEqual(linkOffers(url), streaming, url);
  }
  assert.deepEqual(linkOffers("https://notyoutube.com/a.mp3"), sound);
  assert.match(streamingRefusal, /licensed to use/);
});
