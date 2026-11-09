// Ready (Present's Ready card): what the operator keeps one tap away. Blank
// and Idle always come first; then the service's own list, each a pinned
// item of the order or a deck or media kept only in Ready. Tapping one is
// a side-trip (`Command::ShowReady`): Next past its end goes back to where
// the service was, as `LiveEngine` in lampstand-live does.

import type { Cursor, LoadedItemView, LoadedService, ReadyView, Service } from "./api";

/** An item of the loaded service: in the order, or kept only in Ready. */
export function loadedItem(loaded: LoadedService | null, id: string): LoadedItemView | undefined {
  if (!loaded) return undefined;
  return loaded.items.find((i) => i.id === id) ?? (loaded.ready ?? []).find((r) => r.item.id === id)?.item;
}

/** What the Ready card lists, with nothing to show left out (a pinned item whose file is missing). */
export function readyList(loaded: LoadedService | null): ReadyView[] {
  return (loaded?.ready ?? []).filter((r) => r.item.slides.length > 0 || r.item.sounds.length > 0);
}

/** Whether an item of the order is pinned to Ready, in the saved service. */
export function isPinned(service: Service | null | undefined, item: string): boolean {
  const entry = service?.ready?.[item];
  return entry !== undefined && entry.source === "Order";
}

/** How many things a service keeps ready (pins whose item went are not counted). */
export function readyCount(service: Service | null | undefined): number {
  return Object.entries(service?.ready ?? {}).filter(([id, e]) => e.source !== "Order" || id in service!.items).length;
}

/** The line under On air while a ready item is on a side-trip: where Next goes back to. */
export function returnLine(loaded: LoadedService | null, back: Cursor | null): string | null {
  if (!back) return null;
  const item = loadedItem(loaded, back.item);
  if (!item) return null;
  const slide = item.slides[back.slide];
  return `Next returns to ${item.title}${slide ? ` · ${slide.label ?? back.slide + 1}` : ""}`;
}
