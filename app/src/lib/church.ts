// The church look and idle screen follow the church across its paired
// devices. Plain words for where they stand, free of Svelte and Tauri so
// node can test them.

import type { ChurchStatus, WaitingFile } from "./api";

/** The small note under a page: "Shared with your other devices" once paired, else "This device only". */
export function sharingNote(pairedDevices: number, status: Pick<ChurchStatus, "shared"> | null): string {
  if (pairedDevices === 0) return "This device only. Pair another device in Setup › Devices to share this.";
  return status?.shared === false
    ? "Shared with your other devices once this device has met one of them."
    : "Shared with your other devices.";
}

/** "Waiting for the idle video from Mary's laptop", or `null` when nothing is awaited. */
export function waitingText(waiting: readonly WaitingFile[], only?: readonly string[]): string | null {
  const files = only ? waiting.filter((w) => only.includes(w.what)) : waiting;
  if (files.length === 0) return null;
  const from = [...new Set(files.map((w) => w.from))];
  const what = [...new Set(files.map((w) => w.what))];
  const list = what.length === 1 ? `the ${what[0]}` : `the ${what.slice(0, -1).join(", ")} and ${what[what.length - 1]}`;
  return `Waiting for ${list} from ${from.length === 1 ? from[0] : "your other devices"}`;
}
