// The Audio role on screen: the sound panel in Present and the output
// chooser in Setup › Sound (decision 0010).
//
// The words marked "spec" come from docs/roles.md and decision 0010. The
// ones marked "proposed" are not in the spec and wait for the owner. The
// ones that say why a sound cannot go anywhere ("away", "nobody") are the
// core's own words (the same sentences come back as errors), kept here so a
// screen can show them without a request. Why a sound did not play (the file
// is not there, cannot be read, the output is down) arrives from the core
// already in words, in `SoundView.problem`: those sentences live in one
// place, `problem_words` in app/src-tauri/src/handler/audio.rs.

import type { LoadedItemView, Service, SoundPlaying, SoundStart, SoundView } from "./api";
import { formatLength } from "./loops.ts";
import { lengthLabel, startLabel } from "./sound.ts";

export const words = {
  /** proposed: the panel's heading. */
  title: "Sound",
  /** proposed: the line when nothing plays. */
  quiet: "Nothing is playing.",
  /** proposed: "Plays on Sound desk". */
  playsOn: (holder: string) => `Plays on ${holder}`,
  /** proposed: the buttons. */
  play: "Play",
  pause: "Pause",
  resume: "Resume",
  /** spec: "stop with a fade". */
  fadeOut: "Fade out",
  /** spec: "a panic stop that always works". */
  panic: "Stop the sound now",
  /** proposed: said under a sound that is fading. */
  fading: "Fading out",
  paused: "Paused",
  /** proposed: the heading over the sound the service has. */
  inService: "In this service",
  /** proposed: a backing track's line. */
  trackOf: (song: string) => `Backing track · ${song}`,
  /** The core's words when a sound cannot go anywhere. */
  away: "The device that runs the sound is not connected.",
  nobody: "Nobody runs the sound.",
  /** spec (#182): said in the role bar ahead of a sound nobody will play. */
  nobodyFor: (name: string) => `Nobody runs the sound, so ${name} won't play.`,
  /** proposed (#191): the queue. */
  upNext: "Up next",
  showAll: (n: number) => `Show all (${n})`,
  showLess: "Show less",
  played: (n: number) => `Played (${n})`,
  /** proposed: on a sound the service has reached that waits for Play. */
  waiting: "Waiting for Play",
  /** proposed: a music-only item's line in Present's service order. */
  startsFromNext: "Music only · starts when Next reaches it",
  pressPlay: "Music only · press Play in the Sound box",
  /** proposed: the label on a sound that is playing (was hard-coded in capitals in the panel). */
  playing: "Playing",
  /** proposed: said on the item a problem belongs to, before the core's sentence, for a screen reader. */
  problemOn: (name: string) => `Problem with ${name}`,
  /** proposed: Setup › Sound › Output. */
  outputPanel: "Where the sound comes out",
  outputLabel: "Sound output",
  outputDefault: (name: string | null) => (name ? `This computer's default (${name})` : "This computer's default"),
  outputSentence:
    "Used by the device that runs the sound. Choose the mixer's USB interface here, not the projector's. Changing it stops what is playing.",
  outputMissing: (name: string) => `${name} is not connected to this computer. The default is used until it is.`,
  outputNone: "No sound outputs were found on this computer.",
};

/** One sound a service has, in the order of the service. */
export interface SoundEntry {
  item: string;
  /** A song's backing track rather than a music-only item. */
  backing: boolean;
  name: string;
  /** "Music only · 3:05 · Waits for someone to press play". */
  detail: string;
  start: SoundStart;
}

/** The sound in the service's running order, music-only items and backing tracks, as Present lists them. */
export function soundEntries(items: LoadedItemView[], service: Service | null): SoundEntry[] {
  const out: SoundEntry[] = [];
  for (const item of items) {
    const it = service?.items[item.id];
    if (!it) continue;
    if ("Sound" in it.kind) {
      const s = it.kind.Sound.sound;
      out.push({
        item: item.id,
        backing: false,
        name: s.name,
        detail: [lengthLabel(s.length_ms), startLabel(s.start)].join(" · "),
        start: s.start,
      });
    }
    if (it.backing) {
      out.push({
        item: item.id,
        backing: true,
        name: it.backing.name,
        detail: [words.trackOf(item.title), lengthLabel(it.backing.length_ms), startLabel(it.backing.start)].join(" · "),
        start: it.backing.start,
      });
    }
  }
  return out;
}

/** What the queue needs of a sound: the panel's entries and the phone remote's alike. */
export interface Queued {
  item: string;
  backing: boolean;
  start: SoundStart;
}

/** One sound's place in the queue: a music-only item and its song's backing track each have their own. */
export const entryKey = (e: { item: string; backing: boolean }) => e.item + (e.backing ? "-track" : "");

/**
 * The service's sound as a queue that follows the service (#191). `order`
 * is the service's items in order; `liveItem` is the item on air (null
 * before the service starts). `heard` holds the keys of sounds that have
 * played since their item went live.
 *
 * - `played`: sound the service has passed, greyed and still playable.
 * - `rest`: the sound at the live item and after it.
 * - `upNext`: the first sound after the live item (the first of all before
 *   the service starts).
 * - `waiting`: the sound at the live item that waits for someone to press
 *   Play and has not played: shown and highlighted even in a folded box.
 */
export function soundQueue<T extends Queued>(
  entries: T[],
  order: string[],
  liveItem: string | null,
  heard: ReadonlySet<string> = new Set(),
): { played: T[]; rest: T[]; upNext: T | null; waiting: T | null } {
  const at = liveItem == null ? -1 : order.indexOf(liveItem);
  const place = (e: T) => order.indexOf(e.item);
  const played = entries.filter((e) => at >= 0 && place(e) < at);
  const rest = entries.filter((e) => !played.includes(e));
  const waiting = rest.find((e) => place(e) === at && at >= 0 && e.start === "Waits" && !heard.has(entryKey(e))) ?? null;
  const upNext = rest.find((e) => place(e) > at) ?? null;
  return { played, rest, upNext, waiting };
}

/** What has played since the live item went live, for `soundQueue`'s `heard`. */
export interface Heard {
  at: string | null;
  keys: ReadonlySet<string>;
}

/** `heard` brought up to date: it starts over when another item goes live, and gains what plays now. The same object when nothing changed. */
export function hear(heard: Heard, liveItem: string | null, p: SoundPlaying | null, entries: Queued[]): Heard {
  const base = heard.at === liveItem ? heard : { at: liveItem, keys: new Set<string>() };
  const now = p ? entries.filter((e) => e.item === p.item).map(entryKey) : [];
  if (now.every((k) => base.keys.has(k))) return base;
  return { at: liveItem, keys: new Set([...base.keys, ...now]) };
}

/**
 * The role bar's warning when nobody runs the sound (#182): it names the
 * next sound the service will reach (the one on air first), so it is said
 * ahead of it. Null when someone runs the sound or there is no sound to come.
 */
export function nobodyWarning(view: SoundView | null, queue: { rest: { name: string }[] }): string | null {
  if (!view?.nobody) return null;
  const next = queue.rest[0];
  return next ? words.nobodyFor(next.name) : null;
}

/** "Sound · Playing · Walk-in music": what the folded box says, with the progress and buttons after it. */
export function foldedLine(p: SoundPlaying | null): string {
  if (!p) return words.quiet;
  return `${p.paused ? words.paused : p.ending ? words.fading : words.playing} · ${p.name}`;
}

/** "0:42 left". */
export function leftLabel(p: SoundPlaying, saidAt: number, now: number): string {
  return `${formatLength(Math.max(0, p.length_ms - positionNow(p, saidAt, now)))} left`;
}

/** A music-only item's line in Present's service order: how it will start. */
export function musicLine(item: LoadedItemView): string {
  return item.sounds?.some((s) => !s.backing && s.start === "OnItsOwn") ? words.startsFromNext : words.pressPlay;
}

/** Whether Present shows the sound panel: the service has sound, or sound plays anyway. */
export function showPanel(entries: SoundEntry[], view: SoundView | null): boolean {
  return entries.length > 0 || view?.playing != null;
}

/** Where a sound is now, in milliseconds: the position last said, moved on by the time since unless it is paused. */
export function positionNow(p: SoundPlaying, saidAt: number, now: number): number {
  if (p.paused) return Math.min(p.position_ms, p.length_ms);
  return Math.min(p.position_ms + Math.max(0, now - saidAt), p.length_ms);
}

/** "1:05 of 3:05". */
export function progressLabel(p: SoundPlaying, saidAt: number, now: number): string {
  return `${formatLength(positionNow(p, saidAt, now))} of ${formatLength(p.length_ms)}`;
}

/** A fraction between 0 and 1 for a progress bar. */
export function progressFraction(p: SoundPlaying, saidAt: number, now: number): number {
  return p.length_ms <= 0 ? 0 : positionNow(p, saidAt, now) / p.length_ms;
}

export type Notice = { tone: "warn"; text: string };

/**
 * What to warn about, most important first: why a sound did not play (the
 * core says it in words on every device that shows the sound, including a
 * sound output that is not working), the sound device being away or
 * nobody's. Null when all is well. One message at a time, so a one-device
 * church never sees the same trouble twice.
 */
export function notice(view: SoundView | null): Notice | null {
  if (!view) return null;
  if (view.problem) return { tone: "warn", text: view.problem };
  if (view.away) return { tone: "warn", text: words.away };
  if (view.nobody) return { tone: "warn", text: words.nobody };
  return null;
}

/** The problem that belongs to this entry's item, in words, to show next to it. */
export function entryProblem(view: SoundView | null, entry: SoundEntry): string | null {
  if (!view?.problem || !view.problem_item) return null;
  return view.problem_item === entry.item ? view.problem : null;
}

/**
 * Whether the notice is already shown next to one of the entries, so the
 * panel's own message is then only for a screen reader (said once, not twice).
 */
export function noticeOnEntry(view: SoundView | null, entries: SoundEntry[]): boolean {
  return entries.some((e) => entryProblem(view, e) != null);
}

/** The line under the panel's heading: who plays, or what plays. */
export function whereLine(view: SoundView | null): string | null {
  if (!view || view.here || !view.holder) return null;
  return words.playsOn(view.holder);
}

/** What the transport offers for what is playing now. */
export function transport(p: SoundPlaying | null): { pause: boolean; resume: boolean; fadeOut: boolean } {
  if (!p) return { pause: false, resume: false, fadeOut: false };
  return { pause: !p.paused && !p.ending, resume: p.paused, fadeOut: !p.ending };
}

/** Whether an entry is the sound playing now. */
export function isPlayingEntry(entry: SoundEntry, p: SoundPlaying | null): boolean {
  return p != null && p.item === entry.item;
}

/** The output chosen in Setup, as the list should select it: a name that is no longer on this computer reads as the default. */
export function selectedOutput(chosen: string | null, devices: string[]): string {
  return chosen != null && devices.includes(chosen) ? chosen : "";
}

/** A chosen output that this computer no longer has, for the note under the chooser. */
export function missingOutput(chosen: string | null, devices: string[]): string | null {
  return chosen != null && !devices.includes(chosen) ? chosen : null;
}
