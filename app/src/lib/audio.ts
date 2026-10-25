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

import type { LoadedItemView, Service, SoundPlaying, SoundView } from "./api";
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
      });
    }
    if (it.backing) {
      out.push({
        item: item.id,
        backing: true,
        name: it.backing.name,
        detail: [words.trackOf(item.title), lengthLabel(it.backing.length_ms), startLabel(it.backing.start)].join(" · "),
      });
    }
  }
  return out;
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
