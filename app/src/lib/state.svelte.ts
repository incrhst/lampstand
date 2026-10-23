// State shared across screens, kept current from the event stream: the
// service in the live engine and where the cursors are.

import {
  onEvent,
  request,
  type Cursor,
  type LoadedService,
  type ObsRequest,
  type ObsView,
  type OutputConfig,
  type Overlay,
  type SlideView,
  type DeviceSettings,
  type Theme,
  type UpdateStatus,
} from "./api";
import { loadSettings, updateSettings } from "./settings";
import { sortThemes } from "./themes";

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
  /** OBS: where it is, whether it's connected and streaming (T89). */
  obs: ObsView | null = $state(null);
  /** Where updating Lampstand itself stands; null until first asked. */
  update: UpdateStatus | null = $state(null);
  /** When a new release was last looked for, in ms, so it is not asked too often. */
  updateCheckedAt: number | null = $state(null);
  /** Bumped when a monitor is plugged in or removed, so lists of displays refetch. */
  displaysVersion: number = $state(0);

  /** The library's themes, Starter first. Kept current by `refreshThemes`. */
  themes: Theme[] = $state([]);
  /** This device's settings: default theme, idle screen, church brand. */
  settings: DeviceSettings | null = $state(null);
  /** The engine has nothing on air and no service under way: screens show their idle screen. */
  idle: boolean = $state(true);

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
      live.idle = event.data.idle;
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
      if (event.data.kind === "theme") refreshThemes();
      break;
    case "Obs":
      live.obs = event.data;
      break;
    case "DisplaysChanged":
      live.displaysVersion++;
      break;
  }
});

/** Shows an error from a failed request in the status bar. */
export function report(error: unknown) {
  live.status = error instanceof Error ? error.message : String(error);
}

/** Refetches the library's themes into `live.themes`. */
export async function refreshThemes() {
  try {
    live.themes = sortThemes(await request({ area: "Library", request: "ListThemes" }, "Themes"));
  } catch (e) {
    report(e);
  }
}

/** Refetches this device's settings into `live.settings`. */
export async function refreshSettings() {
  try {
    live.settings = await loadSettings();
  } catch (e) {
    report(e);
  }
}

/** Saves a change to this device's settings and keeps `live.settings` current. */
export async function saveSettings(change: Partial<DeviceSettings>) {
  try {
    live.settings = await updateSettings(change);
  } catch (e) {
    report(e);
  }
}

/** Refetches this device's outputs into `live.outputs`. */
export async function refreshOutputs() {
  try {
    live.outputs = await request({ area: "Outputs", request: "List" }, "Outputs");
  } catch (e) {
    report(e);
  }
}

/**
 * Asks whether a newer release exists. A check that fails is only
 * reported when the operator asked for it (`quiet` false): the background
 * checks stay silent with no network.
 */
export async function checkForUpdate(quiet = true) {
  try {
    live.update = await request({ area: "Update", request: "Check" }, "Update");
    live.updateCheckedAt = Date.now();
  } catch (e) {
    if (!quiet) report(e);
  }
}

/** Where updating stands, without asking the network. */
export async function refreshUpdate() {
  try {
    live.update = await request({ area: "Update", request: "Status" }, "Update");
  } catch (e) {
    report(e);
  }
}

/** Downloads and installs the release found, and restarts. Refused mid-service. */
export async function installUpdate() {
  try {
    live.update = { ...(live.update as UpdateStatus), state: "Installing" };
    live.update = await request({ area: "Update", request: "Install" }, "Update");
  } catch (e) {
    report(e);
    await refreshUpdate();
  }
}

/** Saves an output and keeps `live.outputs` current. */
export async function putOutput(config: OutputConfig) {
  try {
    live.outputs = await request({ area: "Outputs", request: { Put: config } }, "Outputs");
  } catch (e) {
    report(e);
  }
}

/** Removes an output and keeps `live.outputs` current. */
export async function removeOutput(id: string) {
  try {
    live.outputs = await request({ area: "Outputs", request: { Remove: id } }, "Outputs");
  } catch (e) {
    report(e);
  }
}

/**
 * How many OBS pages are showing each picture sent to OBS, by output ID.
 * Kept current by `refreshPictures`, which the screens that show it call
 * every few seconds.
 */
export const pictures: { clients: Record<string, number> } = $state({ clients: {} });

export async function refreshPictures() {
  try {
    const all = await request({ area: "Outputs", request: "BrowserSourceStatus" }, "BrowserSources");
    pictures.clients = Object.fromEntries(all.map((s) => [s.output, s.clients]));
  } catch (e) {
    report(e);
  }
}

/**
 * Sends an OBS request and keeps `live.obs` current with the answer.
 * Throws on failure, like `request`.
 */
export async function obs(req: ObsRequest): Promise<ObsView> {
  const view = await request({ area: "Obs", request: req }, "Obs");
  live.obs = view;
  return view;
}

/** Refetches OBS's state, reporting a failure in the status bar. */
export async function refreshObs() {
  try {
    await obs("Status");
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
