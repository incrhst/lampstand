// Present has two views of the open service. The wall shows every item's
// slides at once, so the operator can reach any slide in a click; classic
// slides shows only the item in view, beside the order of service. The
// choice is remembered on this computer.

import type { Cursor } from "./api";

export type PresentView = "wall" | "classic";

export const VIEW_KEY = "lampstand.present.view";

export const viewOptions = [
  { id: "wall", label: "Wall" },
  { id: "classic", label: "Classic slides" },
] as const;

/** The view chosen on this computer; the wall until another is chosen. Never throws. */
export function loadView(): PresentView {
  try {
    return globalThis.localStorage?.getItem(VIEW_KEY) === "classic" ? "classic" : "wall";
  } catch {
    return "wall";
  }
}

export function saveView(view: PresentView) {
  try {
    globalThis.localStorage?.setItem(VIEW_KEY, view);
  } catch {
    // Not remembered this time.
  }
}

/** How a slide card is marked on the wall: on air, the one Next shows, or neither. */
export function slideMark(item: string, slide: number, live: Cursor | null | undefined, next: Cursor | null | undefined): "live" | "next" | null {
  if (live?.item === item && live.slide === slide) return "live";
  if (next?.item === item && next.slide === slide) return "next";
  return null;
}
