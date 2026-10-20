// What the operator is told about OBS (T89): the status pill in Present
// and, when OBS isn't connected, what's wrong and what to do about it.
// Nothing here is shown unless `view.relevant` (a stream output is on, or
// OBS settings were saved), so churches that don't stream never see OBS.
//
// Graphics reach OBS through the browser source whether or not Lampstand
// is connected to OBS's WebSocket, so the messages say what is lost: OBS
// not running stops the stream's slides; a WebSocket problem only stops
// Lampstand controlling OBS.

import type { ObsOutputView, ObsView } from "./api";

export type PillTone = "ready" | "warn" | "live" | "gold" | "neutral";

export interface ObsPill {
  tone: PillTone;
  text: string;
  /** More detail on hover. */
  title: string;
}

export type ObsAction = "open" | "get" | "setup";

export interface ObsNotice {
  message: string;
  /** What the operator can do: Open OBS, get it, or fix the settings. */
  action: ObsAction;
  /** Step-by-step instructions, when there are some. */
  steps: string[];
}

/** Where "Get OBS" goes. The core opens it (`ObsRequest::Download`). */
export const OBS_DOWNLOAD = "obsproject.com";

export const SERVER_OFF_STEPS = [
  "In OBS, open Tools › WebSocket Server Settings.",
  "Tick Enable WebSocket server and click Apply.",
  "Click Show Connect Info and copy the server password.",
  "Paste it in Setup › OBS here, then Save and test.",
];

/** Skipped frames as a whole-number percentage, or null with none sent. */
export function droppedPercent(stream: ObsOutputView): number | null {
  if (stream.total_frames <= 0) return null;
  return Math.round((stream.skipped_frames / stream.total_frames) * 1000) / 10;
}

/** The pill in Present, or null when OBS isn't relevant here. */
export function obsPill(view: ObsView | null): ObsPill | null {
  if (!view || !view.relevant) return null;
  const c = view.connection;
  if (c === "Connecting") return { tone: "neutral", text: "OBS connecting…", title: `Connecting to OBS at ${where(view)}` };
  if (typeof c === "object" && "Connected" in c) {
    const stream = view.stream;
    const record = view.record;
    const recording = record?.active ? ` · REC ${record.timecode}` : "";
    const version = `OBS ${c.Connected.obs_version} at ${where(view)}`;
    if (stream?.active) {
      if (stream.reconnecting) {
        return { tone: "warn", text: `Stream reconnecting${recording}`, title: `${version}: the stream dropped and OBS is reconnecting it` };
      }
      const dropped = droppedPercent(stream);
      const lossy = dropped !== null && dropped >= 1;
      const frames =
        dropped === null ? "" : `${stream.skipped_frames} of ${stream.total_frames} frames dropped (${dropped}%)`;
      return {
        tone: lossy ? "warn" : "live",
        text: `LIVE ${stream.timecode}${lossy ? ` · ${dropped}% dropped` : ""}${recording}`,
        title: frames ? `${version} · ${frames}` : version,
      };
    }
    if (record?.active) return { tone: "gold", text: `REC ${record.timecode}`, title: `${version} · recording, not streaming` };
    return { tone: "ready", text: "OBS ready", title: `${version} · not streaming` };
  }
  return { tone: "warn", text: "OBS not connected", title: notice(view)?.message ?? "OBS isn't connected" };
}

/** What to tell the operator when OBS isn't connected, or null. */
export function notice(view: ObsView | null): ObsNotice | null {
  if (!view || !view.relevant || !view.problem) return null;
  switch (view.problem) {
    case "NotRunning":
      return { message: "OBS isn't running, so the stream gets no slides.", action: "open", steps: [] };
    case "NotInstalled":
      return {
        message: `Lampstand can't find OBS on this computer. It's free from ${OBS_DOWNLOAD}.`,
        action: "get",
        steps: [],
      };
    case "ServerOff":
      return {
        message: "OBS is running, but Lampstand can't control it: its WebSocket server is off. Slides still reach the stream.",
        action: "setup",
        steps: SERVER_OFF_STEPS,
      };
    case "PasswordRequired":
      return {
        message: "OBS asks for its WebSocket password. Slides still reach the stream.",
        action: "setup",
        steps: SERVER_OFF_STEPS.slice(2),
      };
    case "WrongPassword":
      return {
        message: "OBS refused the WebSocket password. Slides still reach the stream.",
        action: "setup",
        steps: SERVER_OFF_STEPS.slice(2),
      };
    case "Unreachable":
      return {
        message: `OBS at ${where(view)} doesn't answer. Check that it's running and on the same network.`,
        action: "setup",
        steps: [],
      };
  }
}

function where(view: ObsView): string {
  return `${view.host}:${view.port}`;
}

export interface SceneNotice {
  tone: "warn" | "neutral";
  message: string;
  /** Offer Resume: go back to the live item's scene. */
  resume: boolean;
}

/**
 * What to say about the live item's OBS scene (T90): a switch that failed,
 * one waiting for OBS, or a scene picked by hand that is holding. Null when
 * there's nothing to say.
 */
export function sceneNotice(view: ObsView | null): SceneNotice | null {
  if (!view || !view.relevant) return null;
  const p = view.scene_problem;
  if (p) {
    const on = view.program_scene ? `, so it stayed on “${view.program_scene}”` : "";
    if (p.reason === "Missing") {
      return {
        tone: "warn",
        message: `OBS has no scene “${p.scene}”${on}. Check the name in Plan, or add the scene in OBS.`,
        resume: false,
      };
    }
    if (p.reason === "NotConnected") {
      // The connection notice already says OBS is away.
      if (view.problem) return null;
      return { tone: "neutral", message: `OBS switches to “${p.scene}” when it connects.`, resume: false };
    }
    return { tone: "warn", message: `OBS didn't switch to “${p.scene}”: ${p.reason.Failed.message}`, resume: false };
  }
  if (view.held && view.program_scene) {
    return {
      tone: "neutral",
      message: `OBS is on “${view.program_scene}”, picked by hand. The next item switches scenes again.`,
      resume: true,
    };
  }
  return null;
}

// Present's OBS drawer (T91): scene buttons, a live preview and mutes,
// opened from one quiet strip in the bar.

export function isConnected(view: ObsView | null): boolean {
  return !!view && typeof view.connection === "object" && "Connected" in view.connection;
}

/**
 * The scene the next item switches OBS to, for the strip ("Next: Sermon"):
 * the item after the live one (or the first, before the service starts),
 * if it names a scene other than the one on air. Null otherwise: OBS
 * stays where it is.
 */
export function upcomingScene(
  scenes: (string | null | undefined)[],
  liveIndex: number,
  program: string | null,
): string | null {
  const next = scenes[liveIndex + 1]?.trim();
  return next && next !== program ? next : null;
}

/** How often the drawer's live preview asks OBS for a new picture. */
export const PREVIEW_EVERY_MS = 250;

/**
 * Takes pictures one after another, `every` ms apart, until stopped:
 * the next is asked for only once the last has arrived, so a slow OBS
 * is never asked twice at once. A failed picture is reported and the
 * loop carries on. Returns the function that stops it.
 */
export function pollImages(
  take: () => Promise<string>,
  show: (image: string) => void,
  every: number = PREVIEW_EVERY_MS,
  failed: (error: unknown) => void = () => {},
): () => void {
  let stopped = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const tick = async () => {
    try {
      const image = await take();
      if (!stopped) show(image);
    } catch (e) {
      if (!stopped) failed(e);
    }
    if (!stopped) timer = setTimeout(tick, every);
  };
  void tick();
  return () => {
    stopped = true;
    clearTimeout(timer);
  };
}
