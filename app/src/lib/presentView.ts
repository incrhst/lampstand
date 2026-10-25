// The wall in Present marks two slides: the one on air and the one Next
// will show.

import type { Cursor } from "./api";

/** How a slide card is marked on the wall: on air, the one Next shows, or neither. */
export function slideMark(item: string, slide: number, live: Cursor | null | undefined, next: Cursor | null | undefined): "live" | "next" | null {
  if (live?.item === item && live.slide === slide) return "live";
  if (next?.item === item && next.slide === slide) return "next";
  return null;
}
