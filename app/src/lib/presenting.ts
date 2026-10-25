// What this device may do with the screens. The core decides
// (crates/lampstand-sync, presenter.rs and roles.rs); the words are in
// roles.ts.

import type { TeamStatus } from "./api";

/** This device does not run the screens: they stay dark and it cannot take a service live. */
export function isPlanning(team: TeamStatus | null): boolean {
  return team?.presenter.role === "Planning";
}

/** The takeover times offered in Setup › Devices, in seconds. */
export const TAKEOVER_CHOICES = [30, 60, 120, 300, 600, 1800] as const;
