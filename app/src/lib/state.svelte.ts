// State shared across screens, kept current from the event stream: the
// service in the live engine and where the cursors are.

import { onEvent, type Cursor, type LoadedService, type Overlay, type SlideView } from "./api";

class LiveState {
  loaded: LoadedService | null = $state(null);
  live: Cursor | null = $state(null);
  preview: Cursor | null = $state(null);
  overlay: Overlay = $state("None");
  /** Last import message, error or notice, for the status bar. */
  status: string = $state("");
  /** The service open in the planner, kept across screen changes. */
  planning: string | null = $state(null);
  /** Bumped when songs change in bulk (an import), so lists refetch. */
  songsVersion: number = $state(0);

  /** The slide on air, if it is one of the loaded service's slides. */
  liveSlide: SlideView | null = $derived.by(() => slideAt(this.loaded, this.live));
}

function slideAt(loaded: LoadedService | null, cursor: Cursor | null): SlideView | null {
  if (!loaded || !cursor) return null;
  const item = loaded.items.find((i) => i.id === cursor.item);
  return item?.slides[cursor.slide] ?? null;
}

export const live = new LiveState();

onEvent((event) => {
  switch (event.type) {
    case "Live":
      live.live = event.data.live;
      live.preview = event.data.preview;
      live.overlay = event.data.overlay;
      break;
    case "Loaded":
      live.loaded = event.data;
      break;
    case "ImportProgress":
      live.status = event.data.message;
      if (event.data.done === event.data.total) live.songsVersion++;
      break;
  }
});

/** Shows an error from a failed request in the status bar. */
export function report(error: unknown) {
  live.status = error instanceof Error ? error.message : String(error);
}
