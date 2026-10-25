import { test } from "node:test";
import assert from "node:assert/strict";
import type { LoadedItemView, Service, SoundPlaying, SoundView } from "./api.ts";
import {
  entryProblem,
  isPlayingEntry,
  noticeOnEntry,
  missingOutput,
  notice,
  positionNow,
  progressFraction,
  progressLabel,
  selectedOutput,
  showPanel,
  soundEntries,
  transport,
  whereLine,
  words,
} from "./audio.ts";

const sound = (name: string, start: "OnItsOwn" | "Waits" = "Waits", length_ms: number | null = 185_000) => ({
  name,
  file: "a".repeat(64),
  length_ms,
  start,
  keep_off_stream: false,
});

const item = (id: string, title = id): LoadedItemView => ({ id, title, slides: [] });

const service = (): Service =>
  ({
    id: "s",
    title: "Sunday",
    items: {
      a: { kind: { Header: { title: "Welcome" } }, notes: "" },
      b: { kind: { Sound: { sound: sound("Walk-in music") } }, notes: "" },
      c: { kind: { Song: { song: "x", arrangement: null } }, notes: "", backing: sound("Grace (track)", "OnItsOwn", null) },
    },
  }) as unknown as Service;

const view = (over: Partial<SoundView> = {}): SoundView => ({
  here: true,
  holder: "",
  away: false,
  nobody: false,
  output_ok: true,
  playing: null,
  problem: null,
  problem_item: null,
  ...over,
});

const playing = (over: Partial<SoundPlaying> = {}): SoundPlaying => ({
  item: "b",
  name: "Walk-in music",
  paused: false,
  ending: false,
  position_ms: 60_000,
  length_ms: 185_000,
  ...over,
});

test("the panel lists music-only items and backing tracks in the order of the service", () => {
  const entries = soundEntries([item("a"), item("b"), item("c", "Amazing Grace")], service());
  assert.deepEqual(
    entries.map((e) => [e.item, e.backing, e.name]),
    [
      ["b", false, "Walk-in music"],
      ["c", true, "Grace (track)"],
    ],
  );
  assert.equal(entries[0]!.detail, "3:05 · Waits for someone to press play");
  assert.equal(entries[1]!.detail, "Backing track · Amazing Grace · length unknown · Plays on its own");
});

test("a service with no sound lists nothing, and an item the service does not know is skipped", () => {
  assert.deepEqual(soundEntries([item("a")], service()), []);
  assert.deepEqual(soundEntries([item("zzz")], service()), []);
  assert.deepEqual(soundEntries([item("b")], null), []);
});

test("the panel shows only where there is sound to play or sound playing", () => {
  assert.equal(showPanel([], view()), false);
  assert.equal(showPanel([], null), false);
  assert.equal(showPanel([], view({ playing: playing() })), true);
  assert.equal(showPanel(soundEntries([item("b")], service()), view()), true);
});

test("the position moves on with the time, and holds while paused or at the end", () => {
  const p = playing();
  assert.equal(positionNow(p, 1000, 1000), 60_000);
  assert.equal(positionNow(p, 1000, 3500), 62_500);
  assert.equal(positionNow(p, 1000, 500), 60_000, "a clock that went back does not rewind");
  assert.equal(positionNow(p, 1000, 10_000_000), 185_000);
  assert.equal(positionNow(playing({ paused: true }), 1000, 9000), 60_000);
  assert.equal(progressLabel(p, 1000, 3000), "1:02 of 3:05");
  assert.equal(progressFraction(playing({ position_ms: 92_500 }), 0, 0), 0.5);
  assert.equal(progressFraction(playing({ length_ms: 0 }), 0, 0), 0);
});

test("the notice names what is wrong, most important first", () => {
  assert.equal(notice(null), null);
  assert.equal(notice(view()), null);
  assert.deepEqual(notice(view({ problem: "Walk-in music is not on this device yet.", away: true })), {
    tone: "warn",
    text: "Walk-in music is not on this device yet.",
  });
  assert.equal(notice(view({ here: false, away: true, holder: "Sound desk" }))?.text, words.away);
  assert.equal(notice(view({ here: false, nobody: true }))?.text, words.nobody);
  // A sound output that is not working arrives as the core's sentence in `problem`,
  // on this device and on the others alike: the panel has no wording of its own for it.
  const down = "The sound output on this computer isn't working. Check it is plugged in and switched on, or choose another in Setup › Sound.";
  assert.equal(notice(view({ problem: down, output_ok: false }))?.text, down);
  assert.equal(notice(view({ output_ok: false })), null, "the words come from the core, not from the panel");
  assert.equal(notice(view({ here: false, holder: "Sound desk", output_ok: false })), null);
});

test("a problem is shown next to its item, and then the panel's own message is only for a screen reader", () => {
  const entries = soundEntries([item("b"), item("c", "Amazing Grace")], service());
  const said = "Walk-in music couldn't be played. The sound file isn't on Sound desk yet. It will arrive when Sound desk is connected.";
  const v = view({ here: false, holder: "Sound desk", problem: said, problem_item: "b" });
  assert.equal(entryProblem(v, entries[0]!), said);
  assert.equal(entryProblem(v, entries[1]!), null, "only the item it belongs to");
  assert.equal(noticeOnEntry(v, entries), true);
  // A problem about the output as a whole belongs to no item and stays in the panel's message.
  const whole = view({ problem: "The sound output on this computer isn't working.", problem_item: null });
  assert.equal(entryProblem(whole, entries[0]!), null);
  assert.equal(noticeOnEntry(whole, entries), false);
  // An item the service no longer has (the service changed) leaves the message in the panel.
  assert.equal(noticeOnEntry(view({ problem: said, problem_item: "gone" }), entries), false);
  assert.equal(noticeOnEntry(null, entries), false);
  assert.equal(words.playing, "Playing");
});

test("the panel says where the sound plays when it is another device", () => {
  assert.equal(whereLine(view()), null);
  assert.equal(whereLine(view({ here: false, holder: "Sound desk" })), "Plays on Sound desk");
  assert.equal(whereLine(view({ here: false, holder: "" })), null);
});

test("the transport offers what the sound can do now", () => {
  assert.deepEqual(transport(null), { pause: false, resume: false, fadeOut: false });
  assert.deepEqual(transport(playing()), { pause: true, resume: false, fadeOut: true });
  assert.deepEqual(transport(playing({ paused: true })), { pause: false, resume: true, fadeOut: true });
  assert.deepEqual(transport(playing({ ending: true })), { pause: false, resume: false, fadeOut: false });
});

test("an entry is playing when its item is the one playing", () => {
  const [entry] = soundEntries([item("b")], service());
  assert.equal(isPlayingEntry(entry!, playing()), true);
  assert.equal(isPlayingEntry(entry!, playing({ item: "c" })), false);
  assert.equal(isPlayingEntry(entry!, null), false);
});

test("the output list selects the chosen output, or the default when it is gone", () => {
  const devices = ["Built-in", "Mixer USB"];
  assert.equal(selectedOutput("Mixer USB", devices), "Mixer USB");
  assert.equal(selectedOutput(null, devices), "");
  assert.equal(selectedOutput("Old interface", devices), "");
  assert.equal(missingOutput("Old interface", devices), "Old interface");
  assert.equal(missingOutput("Mixer USB", devices), null);
  assert.equal(missingOutput(null, devices), null);
  assert.equal(words.outputDefault("Built-in"), "This computer's default (Built-in)");
  assert.equal(words.outputDefault(null), "This computer's default");
});
