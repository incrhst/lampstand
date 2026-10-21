// The slide the operator sees in the Next pane: where `Next` goes from
// the live cursor, slide by slide (build steps stay on their slide), as
// `LiveEngine::step_forward` in lampstand-live moves. With nothing live,
// the first slide of the service. `nextClick` is what the next click
// shows, build steps included, for the presenter view.

import type { Cursor, LoadedService, SlideView } from "./api";

export type Upcoming = { cursor: Cursor; slide: SlideView; title: string };

export function nextSlide(loaded: LoadedService, live: Cursor | null): Upcoming | null {
  const at = live ? loaded.items.findIndex((i) => i.id === live.item) : -1;
  if (live && at >= 0 && live.slide + 1 < loaded.items[at].slides.length) {
    const item = loaded.items[at];
    return { cursor: { item: item.id, slide: live.slide + 1, step: 0 }, slide: item.slides[live.slide + 1], title: item.title };
  }
  const from = live && at < 0 ? 0 : at + 1;
  const item = loaded.items.slice(from).find((i) => i.slides.length > 0);
  return item ? { cursor: { item: item.id, slide: 0, step: 0 }, slide: item.slides[0], title: item.title } : null;
}

export type NextClick = Upcoming & {
  /** The live slide's next build step rather than another slide. */
  build: boolean;
};

export function nextClick(loaded: LoadedService, live: Cursor | null): NextClick | null {
  const item = live ? loaded.items.find((i) => i.id === live.item) : undefined;
  const slide = live ? item?.slides[live.slide] : undefined;
  if (live && item && slide && live.step < slide.steps) {
    return { cursor: { ...live, step: live.step + 1 }, slide, title: item.title, build: true };
  }
  const next = nextSlide(loaded, live);
  return next ? { ...next, build: false } : null;
}
