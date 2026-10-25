// Sound in the order of the service: music-only items and a song's backing
// track. What the planner says about them, and whether their files are here.
//
// The words marked "spec" come from docs/roles.md and decision 0010. The
// ones marked "proposed" are not in the spec and wait for the owner.

import type { ItemKind, Service, Sound, SoundFile, SoundFileStatus, SoundStart } from "./api";
import { formatLength } from "./loops.ts";

/** What the sound file pickers offer. */
export const audioExtensions = ["mp3", "wav", "m4a", "aac", "flac", "ogg", "opus"];

/** A sound as a file arrives: it waits for somebody to press play. */
export function newSound(file: SoundFile): Sound {
  return { name: file.name, file: file.file, length_ms: file.length_ms, start: "Waits", keep_off_stream: false };
}

/** The music-only item for a file just added. */
export function soundItem(file: SoundFile): ItemKind {
  return { Sound: { sound: newSound(file) } };
}

function extension(name: string): string {
  const dot = name.lastIndexOf(".");
  return dot < 0 ? "" : name.slice(dot + 1).toLowerCase();
}

/** Whether a file name says it is a sound file. */
export function isAudio(name: string): boolean {
  return audioExtensions.includes(extension(name));
}

export const words = {
  /** spec: "music-only item". */
  kind: "Music only",
  /** spec: "backing track". */
  backing: "Backing track",
  /** spec: "plays on its own when the item goes live". */
  onItsOwn: "Plays on its own",
  /** spec: "waits for someone to press play". */
  waits: "Waits for someone to press play",
  /** spec: the "Don't send this sound to the stream" option. */
  keepOffStream: "Don't send this sound to the stream",
  /** proposed: the tag on a card whose sound is kept off the stream. */
  keptOffStreamTag: "Not sent to the stream",
  /** proposed: the file is not on this device. */
  missingTag: "File missing",
  missingComing: "This sound file is not on this device yet. It is being fetched from your other devices.",
  /** proposed: not fetched (a past or undated service, or no other device). */
  missingHere: "This sound file is not on this device.",
  /** proposed: shown on a card that has a backing track. */
  backingTag: "Backing track",
  /** proposed: Setup › Sound. */
  setupTab: "Sound",
  setupTabSubtitle: "Music and backing tracks in services",
  setupTitle: "Sound",
  setupPanel: "In the order of the service",
  setupSwitch: "Use sound in services",
  setupSentence:
    "Adds music-only items and backing tracks to Plan. Sound already in a service always shows, whether this is on or not.",
  /** proposed: the button that adds a music-only item. */
  addSound: "Add sound",
  /** proposed: the picker's title. */
  pickTitle: "Add music or a backing track",
  pickFilter: "Sound",
  /** proposed: the field labels. */
  nameLabel: "Name",
  startLabel: "When it goes live",
  addBacking: "Add a backing track",
  removeBacking: "Remove the backing track",
  removeBackingSure: "Yes, remove the backing track",
  /** proposed: "Sound: 12:30" on the service's heading. */
  total: "Sound",
};

/** How a sound starts, as the volunteer reads it. */
export function startLabel(start: SoundStart): string {
  return start === "OnItsOwn" ? words.onItsOwn : words.waits;
}

/** "3:05", or "length unknown" when the file did not say. */
export function lengthLabel(ms: number | null): string {
  return ms == null ? "length unknown" : formatLength(ms);
}

/** The item's second line: "Music only · 3:05 · Plays on its own". */
export function soundSubtitle(sound: Sound): string {
  return [words.kind, lengthLabel(sound.length_ms), startLabel(sound.start)].join(" · ");
}

/** The sound files of `status` that are not on this device, by item. */
export function missingFiles(status: SoundFileStatus[]): Set<string> {
  return new Set(status.filter((s) => !s.on_device).map((s) => s.item));
}

/** Whether the file of this item's sound (or backing track) is missing. */
export function isMissing(status: SoundFileStatus[], item: string, backing: boolean): boolean {
  return status.some((s) => s.item === item && s.backing === backing && !s.on_device);
}

/** Whether the file of this item's sound is missing and being fetched. */
export function isComing(status: SoundFileStatus[], item: string, backing: boolean): boolean {
  return status.some((s) => s.item === item && s.backing === backing && s.coming);
}

/** Whether a service has any sound in it. */
export function hasSound(service: Service): boolean {
  return Object.values(service.items).some((item) => item && ("Sound" in item.kind || item.backing != null));
}

/**
 * The service's music-only items added up: "Sound: 12:30", or
 * "Sound: at least 12:30" when some file did not give its length. Null when
 * the service has no music-only item, so a service without sound shows
 * nothing.
 */
export function soundTotalLabel(service: Service): string | null {
  let total = 0;
  let known = true;
  let any = false;
  for (const item of Object.values(service.items)) {
    if (!item || !("Sound" in item.kind)) continue;
    any = true;
    const ms = item.kind.Sound.sound.length_ms;
    if (ms == null) known = false;
    else total += ms;
  }
  if (!any) return null;
  return `${words.total}: ${known ? "" : "at least "}${formatLength(total)}`;
}

/**
 * Whether Plan offers to add sound: the device's Setup switch is on, or the
 * service already has some. Sound already in a service is always shown (its
 * items and tracks are data, not an offer); this only decides the buttons
 * that add more.
 */
export function offerSound(service: Service | null, useSound: boolean | undefined): boolean {
  return !!useSound || (service != null && hasSound(service));
}
