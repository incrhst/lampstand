// Lampstand Remote (T100, decision 0006), for phones and tablets: what its
// page shows, worked out from the events the laptop sends. No Tauri here:
// the page runs in a phone's or tablet's browser and talks to the laptop
// over the remote socket. Its name is `REMOTE_NAME` (product.ts).

import type { Cursor, LoadedItemView, LoadedService, Overlay, SlideView, SoundStart } from "./api";
import { nextClick } from "./cursor.ts";
import { REMOTE_NAME } from "./product.ts";
import { lengthLabel } from "./sound.ts";

export const words = {
  title: REMOTE_NAME,
  pairLead: "Type the PIN shown on the laptop.",
  pinLabel: "PIN",
  nameLabel: "Name this phone or tablet",
  pair: "Connect",
  pairing: "Connecting…",
  pairScope: "It will do only what was chosen on the laptop: run the slides and the sound, or page through one speaker's slides.",
  revoked: "This remote was disconnected on the laptop. Ask for a new PIN to connect again.",
  online: "Connected",
  connecting: "Connecting…",
  offline: "Not connected. Trying again…",
  offlineLong:
    "Lost the laptop. Nothing here is up to date until it is back. Check it is on the same Wi-Fi as the laptop.",
  nothingLoaded: "No service is open on the laptop.",
  clearHint: "Take the words off the screen. The background stays.",
};

/** How long to wait before reconnecting after `attempt` failures: 0.5 s doubling to 10 s, with up to a fifth more at random so phones do not all knock at once. */
export function backoff(attempt: number, random: number = Math.random()): number {
  const base = Math.min(10_000, 500 * 2 ** Math.max(0, attempt));
  return Math.round(base * (1 + 0.2 * random));
}

/** A name to start the "Name this phone or tablet" field with, from the browser's user agent. */
export function guessName(userAgent: string): string {
  if (/iPad/.test(userAgent)) return "iPad";
  if (/iPhone/.test(userAgent)) return "iPhone";
  if (/Android/.test(userAgent)) return /Mobile/.test(userAgent) ? "Android phone" : "Android tablet";
  // iPadOS asks for the desktop site and says it is a Mac.
  if (/Macintosh/.test(userAgent) && /Mobile|Safari/.test(userAgent) && !/Chrome/.test(userAgent)) return "iPad";
  return "Phone";
}

/** The digits typed, for the laptop: "482 913" is "482913". */
export function pinDigits(typed: string): string {
  return typed.replace(/\D/g, "").slice(0, 6);
}

/** What a phone is paired for, as the laptop says in its hello (0.9.1). A phone paired before 0.9.1 is an operator's. */
export type PairedAs = { kind: "Operator" } | { kind: "Speaker"; service: string; item: string; title: string };

/** What the laptop says is live, as the remote shows it. */

export type LiveState = {
  live: Cursor | null;
  /** The slide the screens show: `live`, or the slide before a music-only item that is live. */
  shown?: Cursor | null;
  preview: Cursor | null;
  idle: boolean;
  overlay: Overlay;
  /** The operator cleared the words of what is live (0.9.1; absent from an older laptop). */
  cleared?: boolean;
};

export type Shown = { item: LoadedItemView; slide: SlideView; cursor: Cursor };

/** The slide on screen now, if any: while music is live, the slide before it. */
export function onScreen(loaded: LoadedService | null, live: LiveState | null): Shown | null {
  const cursor = live?.shown !== undefined ? live.shown : live?.live;
  if (!loaded || !cursor) return null;
  const item = loaded.items.find((i) => i.id === cursor.item);
  const slide = item?.slides[cursor.slide];
  return item && slide ? { item, slide, cursor } : null;
}

export type Coming = { title: string; slide: SlideView; cursor: Cursor; build: boolean };

/** What Next puts on screen: the next build step of the slide, the next slide, or nothing at the end. */
export function upNext(loaded: LoadedService | null, live: LiveState | null): Coming | null {
  if (!loaded) return null;
  const next = nextClick(loaded, live?.live ?? null);
  return next ? { title: next.title, slide: next.slide, cursor: next.cursor, build: next.build } : null;
}

/** "V1", "John 3:16" or the slide's kind, after the item's title: "Amazing Grace · V1". */
export function slideLine(title: string, slide: SlideView): string {
  return slide.label ? `${title} · ${slide.label}` : title;
}

/** A slide with no text to draw (a deck slide, a picture, a video): the remote asks the laptop for a picture of it. */
export function wantsPicture(slide: SlideView): boolean {
  return slide.lines.length === 0;
}

/** The key a slide picture is kept under on the page: it changes when the service is reloaded. */
export function pictureKey(service: LoadedService, cursor: Cursor): string {
  return `${service.service}/${service.items.length}/${cursor.item}/${cursor.slide}/${cursor.step}`;
}

/** One sound the service has, as the remote's Sound box lists it. */
export interface RemoteSound {
  item: string;
  backing: boolean;
  name: string;
  /** "3:05 · Waits for someone to press play" or "Backing track · Amazing Grace · 3:05". */
  detail: string;
  start: SoundStart;
}

/** The sounds in the order of the service: music-only items and backing tracks. */
export function remoteSounds(loaded: LoadedService | null): RemoteSound[] {
  if (!loaded) return [];
  return loaded.items.flatMap((item) =>
    (item.sounds ?? []).map((s) => ({
      item: item.id,
      backing: s.backing,
      name: s.name,
      detail: s.backing ? `Backing track · ${item.title} · ${lengthLabel(s.length_ms)}` : lengthLabel(s.length_ms),
      start: s.start,
    })),
  );
}
