// The status bar's sync pill: one honest line about this device's sync
// with the devices it is paired with. The backend (`SyncSummary`) decides
// the state; this words it.

import type { SyncSummary } from "./api";

export type SyncPill = {
  tone: "neutral" | "ready" | "warn";
  text: string;
  /** The tooltip: what it means, or the plain reason it failed. */
  title: string;
};

function devices(n: number): string {
  return n === 1 ? "1 device" : `${n} devices`;
}

/** The pill for `sync`; `null` (not asked yet) reads as this device only. */
export function syncPill(sync: SyncSummary | null): SyncPill {
  if (!sync || sync.state === "DeviceOnly") {
    return {
      tone: "neutral",
      text: "This device only",
      title: "Nothing is paired, so changes stay on this device. Open Setup › Devices to pair another",
    };
  }
  switch (sync.state) {
    case "Synced":
      return { tone: "ready", text: `Synced with ${devices(sync.connected)}`, title: "Changes are shared with your paired devices as you make them" };
    case "Syncing":
      return { tone: "ready", text: "Syncing…", title: "Changes are moving to or from a paired device" };
    case "Offline": {
      const away = sync.paired - sync.connected;
      return {
        tone: "warn",
        text: `Offline: ${devices(away)} not reachable`,
        title: "A paired device is off, asleep or on another network. Changes sync when it is back",
      };
    }
    case "Error":
      return { tone: "warn", text: "Sync problem", title: sync.reason ?? "Sync is not working" };
  }
}

/** A failed `Team` request (the sync engine is not running) as a summary. */
export function syncFailed(reason: string): SyncSummary {
  return { state: "Error", paired: 0, connected: 0, reason, last_in_ms: null, last_out_ms: null };
}

/** "just now", "3 minutes ago", for the last change in or out. */
export function ago(ms: number | null, now: number): string {
  if (ms === null) return "none yet";
  const s = Math.max(0, Math.round((now - ms) / 1000));
  if (s < 10) return "just now";
  if (s < 60) return `${s} seconds ago`;
  const m = Math.round(s / 60);
  if (m < 60) return m === 1 ? "1 minute ago" : `${m} minutes ago`;
  const h = Math.round(m / 60);
  return h === 1 ? "1 hour ago" : `${h} hours ago`;
}
