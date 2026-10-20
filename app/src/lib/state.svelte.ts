// State shared across screens, kept current from the event stream: the
// service in the live engine and where the cursors are.

import {
  onEvent,
  request,
  type Cursor,
  type LoadedService,
  type OutputConfig,
  type Overlay,
  type SlideView,
} from "./api";

class LiveState {
  loaded: LoadedService | null = $state(null);
  live: Cursor | null = $state(null);
  preview: Cursor | null = $state(null);
  overlay: Overlay = $state("None");
  /** Last import message, error or notice, for the status bar. */
  status: string = $state("");
  /** A running import, for the status bar's progress bar. */
  progress: { done: number; total: number } | null = $state(null);
  /** This device's outputs, for the status bar and the operator view. */
  outputs: OutputConfig[] = $state([]);
  /** The service open in the planner, kept across screen changes. */
  planning: string | null = $state(null);
  /** Bumped when songs change in bulk (an import), so lists refetch. */
  songsVersion: number = $state(0);
  /** Bumped when a deck is added or changes (here or synced in). */
  decksVersion: number = $state(0);

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
      if (event.data.done === event.data.total) {
        live.progress = null;
        live.songsVersion++;
      } else {
        live.progress = { done: event.data.done, total: event.data.total };
      }
      break;
    case "Changed":
      if (event.data.kind === "deck") live.decksVersion++;
      break;
  }
});

/** Shows an error from a failed request in the status bar. */
export function report(error: unknown) {
  live.status = error instanceof Error ? error.message : String(error);
}

/** Refetches this device's outputs into `live.outputs`. */
export async function refreshOutputs() {
  try {
    live.outputs = await request({ area: "Outputs", request: "List" }, "Outputs");
  } catch (e) {
    report(e);
  }
}

/** The output the operator view names and draws the safe area of. */
export function mainOutput(outputs: OutputConfig[]): OutputConfig | null {
  const audience = outputs.filter((o) => typeof o.role === "object" && "Audience" in o.role);
  return (
    audience.find((o) => typeof o.role === "object" && o.role.Audience.primary) ?? audience[0] ?? outputs[0] ?? null
  );
}

/**
 * Whether an output has had its safe area set. Outputs keep no record of
 * when they were calibrated, so an output with no margins counts as not
 * calibrated (a screen that truly shows every pixel can set a tiny one).
 */
export function isCalibrated(output: OutputConfig): boolean {
  const s = output.safe_area;
  return s.top > 0 || s.right > 0 || s.bottom > 0 || s.left > 0;
}
