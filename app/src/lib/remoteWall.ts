// Lampstand Remote's wall (0.9.1): the whole service on a phone or tablet,
// as Present's wall shows it on the laptop. Every item in order under its
// group heading, each with its slides as cards marked LIVE and NEXT. Worked
// out here from what the page already receives (`Event::Loaded` and
// `Event::Live`), so the page only draws it. No Tauri and no DOM: the tests
// run this in Node.

import type { Cursor, LoadedItemView, LoadedService, SlideView, SoundStart } from "./api";
import { nextSlide } from "./cursor.ts";
import { slideMark } from "./presentView.ts";
import { onScreen, wantsPicture, type LiveState } from "./remote.ts";
import { lengthLabel } from "./sound.ts";

export const wallWords = {
  live: "LIVE",
  idle: "IDLE",
  blank: "BLANK",
  next: "Next",
  nextLeft: (n: number) => `Next · ${n} left`,
  back: "Back",
  previous: "Previous",
  blankButton: "Blank",
  idleButton: "Idle",
  clear: "Clear",
  showThis: "Show this slide",
  cancel: "Cancel",
  selected: "Selected:",
  onAir: "On air",
  slides: (n: number) => `${n} ${n === 1 ? "slide" : "slides"}`,
  builds: (n: number) => `${n} ${n === 1 ? "build" : "builds"}`,
  build: (k: number, n: number) => `build ${k} of ${n}`,
  returnsTo: (where: string) => `Show returns to ${where}`,
  nothingYet: "Nothing on air yet. Next shows the first slide.",
  idleScreen: "Idle screen",
  blankScreen: "The screen is blank",
  musicOnly: "Music only",
  nothingToShow: "Nothing to show",
  waitsForPlay: "Waits for Play",
  playsOnItsOwn: "Plays on its own",
  service: "The whole service",
};

/** A slide card on the wall. */
export interface WallCard {
  cursor: Cursor;
  slide: SlideView;
  /** "V1", "Slide 3", or the slide's number when it has no label. */
  label: string;
  mark: "live" | "next" | null;
  /** A slide with no text (a deck slide, a picture): the page asks the laptop for a picture of it. */
  picture: boolean;
}

/** A music-only item, drawn as a dashed sound card. */
export interface WallSound {
  name: string;
  /** "Waits for Play" or "Plays on its own". */
  start: string;
  /** "3:05", or "length unknown". */
  length: string;
}

export interface WallRow {
  item: LoadedItemView;
  /** The item's place in the order of service, counted as Present counts it. */
  number: number;
  onAir: boolean;
  /** Before the item on air. */
  played: boolean;
  /** "3 slides", "On air", "Music only" or "Nothing to show". */
  subtitle: string;
  cards: WallCard[];
  sound: WallSound | null;
}

export interface WallGroup {
  /** The group heading, or null for the items before the first one. */
  heading: { header: string; title: string } | null;
  rows: WallRow[];
}

function startWords(start: SoundStart): string {
  return start === "OnItsOwn" ? wallWords.playsOnItsOwn : wallWords.waitsForPlay;
}

/** The item's sound card: its own sound, for an item with nothing to show. */
export function wallSound(item: LoadedItemView): WallSound | null {
  if (item.slides.length > 0) return null;
  const own = (item.sounds ?? []).find((s) => !s.backing);
  return own ? { name: own.name, start: startWords(own.start), length: lengthLabel(own.length_ms) } : null;
}

/**
 * The wall: every item in order, under its group heading. The live item is
 * the one the live cursor is on (a music-only item too); NEXT marks the
 * slide Next shows, as on the laptop.
 */
export function wallGroups(loaded: LoadedService | null, live: LiveState | null): WallGroup[] {
  if (!loaded) return [];
  const cursor = live?.live ?? null;
  const next = nextSlide(loaded, cursor)?.cursor ?? null;
  const liveIndex = cursor ? loaded.items.findIndex((i) => i.id === cursor.item) : -1;
  const headings = new Map(loaded.sections.map((s) => [s.header, s]));
  const groups: WallGroup[] = [];
  loaded.items.forEach((item, i) => {
    const section = headings.get(item.id);
    if (section) {
      groups.push({ heading: { header: section.header, title: section.title }, rows: [] });
      return;
    }
    if (groups.length === 0) groups.push({ heading: null, rows: [] });
    const onAir = i === liveIndex;
    const sound = wallSound(item);
    const n = item.slides.length;
    groups[groups.length - 1].rows.push({
      item,
      number: i + 1,
      onAir,
      played: liveIndex >= 0 && i < liveIndex,
      subtitle: onAir ? wallWords.onAir : n > 0 ? wallWords.slides(n) : sound ? wallWords.musicOnly : wallWords.nothingToShow,
      sound,
      cards: item.slides.map((slide, s) => ({
        cursor: { item: item.id, slide: s, step: 0 },
        slide,
        label: slide.label ?? String(s + 1),
        mark: slideMark(item.id, s, cursor, next),
        picture: wantsPicture(slide),
      })),
    });
  });
  return groups;
}

/** What the top strip says. */
export interface Status {
  tone: "live" | "idle" | "blank";
  /** LIVE, IDLE or BLANK. */
  label: string;
  /** The item on air (bold), if any. */
  item: string | null;
  /** Its slide label, "build k of n" during builds, or the line under Idle. */
  detail: string;
}

/** LIVE, IDLE or BLANK, the item on air and its slide. Under Idle: "Show returns to {item} · {slide}". */
export function status(loaded: LoadedService | null, live: LiveState | null): Status {
  const overlay = live?.overlay ?? "None";
  const shown = onScreen(loaded, live);
  const cursor = live?.live ?? null;
  const liveItem = cursor && loaded ? loaded.items.find((i) => i.id === cursor.item) : undefined;
  const liveSlide = cursor ? liveItem?.slides[cursor.slide] : undefined;
  const where = liveItem ? (liveSlide ? `${liveItem.title} · ${liveSlide.label ?? cursor!.slide + 1}` : liveItem.title) : null;
  if (overlay === "Black") return { tone: "blank", label: wallWords.blank, item: liveItem?.title ?? null, detail: wallWords.blankScreen };
  if (overlay === "Idle" || live?.idle || !shown) {
    return { tone: "idle", label: wallWords.idle, item: null, detail: where ? wallWords.returnsTo(where) : wallWords.nothingYet };
  }
  const label = shown.slide.label ?? String(shown.cursor.slide + 1);
  const steps = shown.slide.steps;
  const step = shown.cursor.step;
  // While music is live the screens keep the slide before it: say what plays.
  const music = liveItem && liveItem.id !== shown.item.id ? ` · ${liveItem.title}` : "";
  return {
    tone: "live",
    label: wallWords.live,
    item: shown.item.title,
    detail: steps > 0 && step > 0 ? `${label} · ${wallWords.build(step, steps)}${music}` : `${label}${music}`,
  };
}

/** Next's words: "Next · 2 left" while a deck slide with builds still to come is on air. */
export function nextLabel(loaded: LoadedService | null, live: LiveState | null): string {
  const cursor = live?.live;
  const slide = cursor && loaded?.items.find((i) => i.id === cursor.item)?.slides[cursor.slide];
  if (!cursor || !slide || !wantsPicture(slide) || slide.steps === 0) return wallWords.next;
  const left = slide.steps - cursor.step;
  return left > 0 ? wallWords.nextLeft(left) : wallWords.next;
}

const same = (a: Cursor | null, b: Cursor | null) => !!a && !!b && a.item === b.item && a.slide === b.slide && a.step === b.step;

/**
 * The two-step tap. A first tap on a card selects it and shows nothing; a
 * tap on the selected card again shows it. "Show this slide" shows the
 * selection; Cancel and any live change drop it.
 */
export function tap(selected: Cursor | null, card: Cursor): { selected: Cursor | null; show: Cursor | null } {
  return same(selected, card) ? { selected: null, show: card } : { selected: card, show: null };
}

/** A key that changes whenever what is live changes: a cursor, Blank, Idle, or the link dropping. */
export function liveKey(live: LiveState | null): string {
  if (!live) return "";
  const c = live.live;
  return `${c ? `${c.item}/${c.slide}/${c.step}` : "-"}|${live.overlay}|${live.idle}`;
}

/** The selection, if its slide is still in the service (a reload can take it away). */
export function keepSelection(loaded: LoadedService | null, selected: Cursor | null): Cursor | null {
  if (!selected || !loaded) return null;
  return loaded.items.find((i) => i.id === selected.item)?.slides[selected.slide] ? selected : null;
}

/** "Amazing Grace · V2", for the bar while a card is selected. */
export function selectionLine(loaded: LoadedService | null, selected: Cursor | null): string {
  const item = selected && loaded?.items.find((i) => i.id === selected.item);
  const slide = item && item.slides[selected!.slide];
  if (!item || !slide) return "";
  return `${item.title} · ${slide.label ?? selected!.slide + 1}`;
}

/** Phone upright (strips that swipe, the bar at the foot) or wide (the desktop wall, the bar along the top). */
export type Layout = "narrow" | "wide";

export const WIDE_FROM = 700;

export function layoutFor(width: number): Layout {
  return width >= WIDE_FROM ? "wide" : "narrow";
}

/** Touch screens and phones take two taps to show a slide; a mouse on a wide screen takes one, as on the laptop. */
export function twoStepTap(coarse: boolean, layout: Layout): boolean {
  return coarse || layout === "narrow";
}

/** Width of the slide pictures the page asks the laptop for: small, for church Wi-Fi. */
export const PICTURE_WIDTH = 240;
/** Pictures asked for at once. */
export const PICTURES_AT_ONCE = 2;

/**
 * Slide pictures, asked for a few at a time and kept. `want` with a lower
 * priority goes first (the slide on air is 0); asking again for one that
 * waits only moves it up. A picture the laptop could not give is not asked
 * for again until `retry` (after a reconnect). `reset` starts afresh (a
 * different service): answers to requests made before it are dropped.
 */
export class PictureQueue<Job> {
  private done = new Map<string, string>();
  private failed = new Set<string>();
  private running = new Set<string>();
  private waiting: { key: string; job: Job; priority: number; order: number }[] = [];
  private order = 0;
  private generation = 0;
  /** Requests out now, old ones after a reset included, so the cap holds. */
  private inflight = 0;

  private fetch: (job: Job) => Promise<string | null>;
  private onpicture: (key: string, picture: string) => void;
  private limit: number;

  constructor(fetch: (job: Job) => Promise<string | null>, onpicture: (key: string, picture: string) => void, limit = PICTURES_AT_ONCE) {
    this.fetch = fetch;
    this.onpicture = onpicture;
    this.limit = limit;
  }

  get(key: string): string | undefined {
    return this.done.get(key);
  }

  /** How many requests are out now. */
  get busy(): number {
    return this.inflight;
  }

  want(key: string, job: Job, priority: number) {
    if (this.done.has(key) || this.running.has(key) || this.failed.has(key)) return;
    const queued = this.waiting.find((w) => w.key === key);
    if (queued) {
      queued.priority = Math.min(queued.priority, priority);
    } else {
      this.waiting.push({ key, job, priority, order: this.order++ });
    }
    this.pump();
  }

  /** Something no longer in view: it waits no more, unless it is already being fetched. */
  drop(key: string) {
    this.waiting = this.waiting.filter((w) => w.key !== key);
  }

  retry() {
    this.failed.clear();
  }

  reset() {
    this.generation++;
    this.done.clear();
    this.failed.clear();
    this.running.clear();
    this.waiting = [];
  }

  private pump() {
    while (this.inflight < this.limit && this.waiting.length > 0) {
      this.waiting.sort((a, b) => a.priority - b.priority || a.order - b.order);
      const { key, job } = this.waiting.shift()!;
      this.running.add(key);
      this.inflight++;
      const generation = this.generation;
      this.fetch(job)
        .then(
          (picture) => {
            if (generation !== this.generation) return;
            if (picture) {
              this.done.set(key, picture);
              this.onpicture(key, picture);
            } else this.failed.add(key);
          },
          () => {
            if (generation === this.generation) this.failed.add(key);
          },
        )
        .finally(() => {
          this.inflight--;
          if (generation === this.generation) this.running.delete(key);
          this.pump();
        });
    }
  }
}

/** What the wall is made of, so the page keeps its pictures when the laptop says the same service again (a reconnect). */
export function serviceShape(loaded: LoadedService | null): string {
  if (!loaded) return "";
  return `${loaded.service}:${loaded.items.map((i) => `${i.id}=${i.slides.map((s) => `${s.label ?? ""}+${s.steps}`).join(",")}`).join(";")}`;
}
