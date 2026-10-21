// How the screen talks about updating Lampstand: the status line, whether
// the Install button works, and when to look for a new release. Kept free
// of Svelte and Tauri so node can test it.

import type { UpdateState, UpdateStatus } from "./api";

/** Look for a new release this long after starting, so startup is not slowed by it. */
export const FIRST_CHECK_MS = 20_000;
/** And again this often while the app stays open (it is often left open all week). */
export const CHECK_EVERY_MS = 6 * 60 * 60 * 1000;

export interface UpdateSummary {
  tone: "ready" | "warn" | "gold" | "neutral";
  text: string;
}

export function availableVersion(state: UpdateState): string | null {
  return typeof state === "object" && "Available" in state ? state.Available.version : null;
}

/** The status line: where updating stands, in a few words. */
export function updateSummary(status: UpdateStatus | null): UpdateSummary {
  if (!status) return { tone: "neutral", text: "Not checked yet" };
  const s = status.state;
  if (s === "NotChecked") return { tone: "neutral", text: "Not checked yet" };
  if (s === "UpToDate") return { tone: "ready", text: "Up to date" };
  if (s === "Installing") return { tone: "neutral", text: "Installing…" };
  if ("Available" in s) return { tone: "gold", text: `Version ${s.Available.version} is available` };
  return { tone: "warn", text: `Could not check: ${s.Failed.message}` };
}

/** The Install button works: a release is waiting and nothing is in its way. */
export function canInstall(status: UpdateStatus | null): boolean {
  return !!status && availableVersion(status.state) !== null && status.blocked === null;
}

/** Whether it is time to look again: never checked, or the last check is old. */
export function checkDue(lastCheckedAt: number | null, now: number): boolean {
  return lastCheckedAt === null || now - lastCheckedAt >= CHECK_EVERY_MS;
}
