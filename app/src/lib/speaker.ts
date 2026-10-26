// A speaker's phone (0.9.1, decision 0006 amended): one item's slides,
// with the speaker's notes and a timer, and Next and Back that work only
// while that item is on the screen. Worked out here from the events the
// page already gets (`Event::Loaded`, `Event::Live`); the laptop's gate
// decides what a press may do, and refuses Next past the item's last slide
// and Back before its first, whatever this page shows.

import type { Cursor, LoadedItemView, LoadedService, SlideView } from "./api";
import type { LiveState } from "./remote.ts";

export const speakerWords = {
  yourSlides: "Your slides",
  waiting: "Waiting for your slides",
  waitingLead: (item: string) => `Next and Back work when the service team puts “${item}” on the screen.`,
  onScreenNow: (what: string) => `On the screen now: ${what}`,
  nothingOnScreen: "On the screen now: nothing yet",
  cleared: "The service team has cleared the screen.",
  clearedLead: "Next and Back work when your slides are back.",
  live: "LIVE",
  blank: "BLANK",
  blankButton: "Blank",
  slideOf: (n: number, total: number) => `Slide ${n} of ${total}`,
  buildOf: (k: number, m: number) => `build ${k} of ${m}`,
  nextClick: "Next click",
  nextSlide: "Next slide",
  clicksLeft: (n: number) => `${n} ${n === 1 ? "click" : "clicks"} left on this slide`,
  last: "Last slide. The service team takes it from here.",
  notes: "Notes",
  noNotes: "No notes on this slide.",
  next: "Next",
  back: "Back",
  gone: "This item is no longer in the service.",
  // The Connect dialog on the laptop.
  forTitle: "What is it for?",
  runTitle: "Run the service",
  runLead: "The whole service as cards: Next, Back, Blank, Idle and the sound.",
  speakerTitle: "Speaker",
  speakerLead: "One item's slides with notes and a timer. Next and Back only, only while it's on the screen.",
  whose: "Whose slides?",
  scanSpeaker: "The speaker scans this with their phone",
  speakerNote: (item: string) =>
    `The speaker's phone shows only “${item}”. It stops working when the service is closed, and you can still run everything from here.`,
  runNote: "It joins the paired phones and tablets in Setup › Devices, where you can disconnect it at any time.",
  noItems: "Open a service with slides to connect a speaker.",
};

/** A deck's slide: no text of its own, called "Slide n" (as Present tells them). */
export function isDeckSlide(slide: SlideView): boolean {
  return slide.lines.length === 0 && /^Slide \d+$/.test(slide.label ?? "");
}

/** The items a speaker's phone can be paired for: those with slides. */
export function speakerChoices(loaded: LoadedService | null): LoadedItemView[] {
  return (loaded?.items ?? []).filter((i) => i.slides.length > 0);
}

/** The item the dialog offers first: the first deck after the live item, else the first deck, else the first item with slides. */
export function defaultSpeakerItem(loaded: LoadedService | null, liveItem: string | null): string | null {
  const items = loaded?.items ?? [];
  const deck = (i: LoadedItemView) => i.slides.some(isDeckSlide);
  const at = liveItem ? items.findIndex((i) => i.id === liveItem) : -1;
  const after = at >= 0 ? items.slice(at + 1).find(deck) : undefined;
  return (after ?? items.find(deck) ?? speakerChoices(loaded)[0])?.id ?? null;
}

/** What comes after the slide on screen, inside the speaker's item. */
export type SpeakerNext =
  | { kind: "click"; cursor: Cursor; slide: SlideView; clicksLeft: number }
  | { kind: "slide"; cursor: Cursor; slide: SlideView; title: string };

export type SpeakerView =
  | { state: "gone" }
  /** Its item is live, but the service team cleared the screen. */
  | { state: "cleared"; title: string }
  | {
      state: "waiting";
      title: string;
      /** "Amazing Grace · V2", or null when nothing is on air. */
      onScreen: string | null;
    }
  | {
      state: "live";
      title: string;
      blank: boolean;
      cursor: Cursor;
      slide: SlideView;
      /** "Slide 2 of 5 · build 1 of 3". */
      position: string;
      next: SpeakerNext | null;
      /** On the last slide, fully built: Next is greyed. */
      last: boolean;
      /** On the first slide, before any build: Back is greyed. */
      first: boolean;
      notes: string;
    };

const labelOf = (slide: SlideView, n: number) => slide.label ?? slide.lines[0] ?? String(n + 1);

/** The speaker's page for item `own`, from what the laptop says. */
export function speakerView(loaded: LoadedService | null, live: LiveState | null, own: string): SpeakerView {
  const item = loaded?.items.find((i) => i.id === own);
  if (!loaded || !item) return { state: "gone" };
  const cursor = live?.live ?? null;
  if (!cursor || cursor.item !== own || live?.overlay === "Idle") {
    const on = cursor ? loaded.items.find((i) => i.id === cursor.item) : undefined;
    const onSlide = on && cursor ? on.slides[cursor.slide] : undefined;
    return {
      state: "waiting",
      title: item.title,
      onScreen: on ? (onSlide ? `${on.title} · ${labelOf(onSlide, cursor!.slide)}` : on.title) : null,
    };
  }
  const slide = item.slides[cursor.slide];
  if (!slide) return { state: "waiting", title: item.title, onScreen: item.title };
  // The operator took the words off the screen: Next and Back would put
  // them back, so the laptop refuses them until the slides are back.
  if (live?.cleared) return { state: "cleared", title: item.title };
  const steps = slide.steps;
  const position =
    steps > 0
      ? `${speakerWords.slideOf(cursor.slide + 1, item.slides.length)} · ${speakerWords.buildOf(cursor.step, steps)}`
      : speakerWords.slideOf(cursor.slide + 1, item.slides.length);
  let next: SpeakerNext | null = null;
  if (cursor.step < steps) {
    next = { kind: "click", cursor: { ...cursor, step: cursor.step + 1 }, slide, clicksLeft: steps - cursor.step };
  } else if (cursor.slide + 1 < item.slides.length) {
    const after = item.slides[cursor.slide + 1];
    next = { kind: "slide", cursor: { item: own, slide: cursor.slide + 1, step: 0 }, slide: after, title: labelOf(after, cursor.slide + 1) };
  }
  return {
    state: "live",
    title: item.title,
    blank: live?.overlay === "Black",
    cursor,
    slide,
    position,
    next,
    last: next === null,
    first: cursor.slide === 0 && cursor.step === 0,
    notes: slide.notes,
  };
}

/**
 * When the speaker's item went on air, for the timer: kept while it stays
 * on air, set afresh each time it goes live again, and null while it is not.
 */
export function liveSince(previous: number | null, onAir: boolean, now: number): number | null {
  if (!onAir) return null;
  return previous ?? now;
}

/** "4:05" or "1:02:09". */
export function elapsed(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = String(s % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${sec}` : `${m}:${sec}`;
}

/** The time of day, as the phone's clock shows it: "10:42". */
export function clock(date: Date): string {
  return `${date.getHours()}:${String(date.getMinutes()).padStart(2, "0")}`;
}
