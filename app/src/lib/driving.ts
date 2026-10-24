// Driving, in words. The core decides (crates/lampstand-sync, driver.rs):
// while one device presents, another paired device may operate it from its
// own screen. This turns what it reports into the labels, prompts and
// choices the screens show.

import type { DeviceView, DrivingView, TeamStatus } from "./api";

/** What a device's role reads as in Setup › Devices and the status bar. */
export type DeviceRole = "Presenting" | "Driving" | "Planning";

/** The presenter this device is driving, whether or not it is connected now. */
export function drivingFrom(team: TeamStatus | null): DrivingView | null {
  return team?.presenter.drive?.driving ?? null;
}

/** This device is driving another: its Present screen mirrors the presenter. */
export function isDriving(team: TeamStatus | null): boolean {
  return drivingFrom(team) !== null;
}

/** Driving, but the presenter is not connected: the controls wait. */
export function driverOffline(team: TeamStatus | null): boolean {
  const d = drivingFrom(team);
  return d !== null && !d.online;
}

/** Another device is operating this one, the presenter. */
export function drivenBy(team: TeamStatus | null): string | null {
  if (!team || team.presenter.role !== "Presenting") return null;
  return team.presenter.drive?.active?.name ?? null;
}

/** The role of a paired device, as this device knows it. */
export function roleOf(team: TeamStatus, device: DeviceView): DeviceRole {
  if (device.presenting) return "Presenting";
  if (team.presenter.drive?.active?.device === device.device) return "Driving";
  return "Planning";
}

/** This device's own role. */
export function ownRole(team: TeamStatus | null): DeviceRole {
  if (!team || team.presenter.role === "Presenting") return "Presenting";
  return isDriving(team) ? "Driving" : "Planning";
}

/** Setup › Devices: what a paired device is doing, with the other end named. */
export function roleLine(team: TeamStatus, device: DeviceView): string {
  const role = roleOf(team, device);
  if (role === "Driving") return `Driving ${team.presenter.holder_name || "the presenter"}`;
  if (role === "Presenting") {
    const by = team.presenter.drive?.active?.name;
    return by ? `Presenting · driven from ${by}` : "Presenting";
  }
  return "Planning";
}

/** Setup › Devices: this device's role in one line. */
export function ownRoleLine(team: TeamStatus): string {
  const p = team.presenter;
  switch (ownRole(team)) {
    case "Presenting": {
      const by = drivenBy(team);
      return by ? `Presenting · driven from ${by}` : "Presenting";
    }
    case "Driving":
      return `Driving ${drivingFrom(team)?.name ?? p.holder_name}`;
    default:
      return "Planning";
  }
}

/** What a driver's bar says. */
export function driveBarText(d: DrivingView): string {
  return d.online
    ? `Driving ${d.name}`
    : `${d.name} is not connected. Controls wait until it is back.`;
}

/** What the presenting device's bar says while a device drives it. */
export function drivenBarText(by: string): string {
  return `${by} is driving. Your keyboard and clicker still work.`;
}

/** Said once on a device that was driving when the presenter took control back. */
export function takenBackNotice(by: string): string {
  return `${by} took control back. This device is no longer driving.`;
}
