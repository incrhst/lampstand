// Running the service from another device, in state. The core decides
// (crates/lampstand-sync, driver.rs and roles.rs): while one device runs the
// screens, another paired device may hold the Operator seat and run the
// service from its own screen. The words are in roles.ts.

import type { DrivingView, TeamStatus } from "./api";

/** The device whose screens this one runs the service for, connected or not. */
export function drivingFrom(team: TeamStatus | null): DrivingView | null {
  return team?.presenter.drive?.driving ?? null;
}

/** This device runs the service for another's screens: its Present screen mirrors them. */
export function isDriving(team: TeamStatus | null): boolean {
  return drivingFrom(team) !== null;
}

/** The device that runs the screens is not connected: the service controls wait. */
export function driverOffline(team: TeamStatus | null): boolean {
  const d = drivingFrom(team);
  return d !== null && !d.online;
}

/** Another device runs the service for this one, which runs the screens. */
export function drivenBy(team: TeamStatus | null): string | null {
  if (!team || team.presenter.role !== "Presenting") return null;
  return team.presenter.drive?.active?.name ?? null;
}
