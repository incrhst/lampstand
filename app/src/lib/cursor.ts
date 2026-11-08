// The slide the operator sees in the Next pane: where `Next` goes from
// the live cursor, slide by slide (build steps stay on their slide), as
// `LiveEngine::step_forward` in lampstand-live moves. With nothing live,
// the first slide of the service. `nextClick` is what the next click
// shows, build steps included, for the presenter view. On a side-trip to
// a ready item (`back` is where the service was), Next past the item's end
// goes back there.

import type { Cursor, LoadedService, SlideView } from "./api";
import { loadedItem } from "./ready.ts";

export type Upcoming = { cursor: Cursor; slide: SlideView; title: string };

export function nextSlide(loaded: LoadedService, live: Cursor | null, back: Cursor | null = null): Upcoming | null {
  const at = live ? loaded.items.findIndex((i) => i.id === live.item) : -1;
  const current = live ? loadedItem(loaded, live.item) : undefined;
  if (live && current && live.slide + 1 < current.slides.length) {
    return { cursor: { item: current.id, slide: live.slide + 1, step: 0 }, slide: current.slides[live.slide + 1], title: current.title };
  }
  if (live && back) {
    const item = loadedItem(loaded, back.item);
    const slide = item?.slides[back.slide];
    if (item && slide) return { cursor: back, slide, title: item.title };
  }
  const from = live && at < 0 ? 0 : at + 1;
  const item = loaded.items.slice(from).find((i) => i.slides.length > 0);
  return item ? { cursor: { item: item.id, slide: 0, step: 0 }, slide: item.slides[0], title: item.title } : null;
}

export type NextClick = Upcoming & {
  /** The live slide's next build step rather than another slide. */
  build: boolean;
};

export function nextClick(loaded: LoadedService, live: Cursor | null, back: Cursor | null = null): NextClick | null {
  const item = live ? loadedItem(loaded, live.item) : undefined;
  const slide = live ? item?.slides[live.slide] : undefined;
  if (live && item && slide && live.step < slide.steps) {
    return { cursor: { ...live, step: live.step + 1 }, slide, title: item.title, build: true };
  }
  const next = nextSlide(loaded, live, back);
  return next ? { ...next, build: false } : null;
}

/**
 * The next (`1`) or previous (`-1`) item in the running order that has
 * something to show, from `from` (the item being looked at). With none
 * chosen, "next" is the first item and "previous" is nowhere. Null at
 * either end. This only chooses: it does not put anything on air.
 */
export function neighbourItem(items: { id: string; slides: unknown[] }[], from: string | null, direction: 1 | -1): string | null {
  const at = from === null ? -1 : items.findIndex((i) => i.id === from);
  const candidates =
    direction === 1 ? items.slice(at + 1) : at < 0 ? [] : items.slice(0, at).reverse();
  return candidates.find((i) => i.slides.length > 0)?.id ?? null;
}
