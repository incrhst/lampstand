// Who may present, in words. The core decides (crates/lampstand-sync,
// presenter.rs); this turns what it reports into the labels, prompts and
// choices the screens show.

import type { PresenterView, TeamStatus } from "./api";

/** This device only plans: its screens stay dark and it cannot take a service live. */
export function isPlanning(team: TeamStatus | null): boolean {
  return team?.presenter.role === "Planning";
}

/** "30 seconds", "1 minute", "5 minutes", "2 hours": how long, for people. */
export function howLong(seconds: number): string {
  const s = Math.max(0, Math.round(seconds));
  if (s < 60) return s === 1 ? "1 second" : `${s} seconds`;
  const m = Math.round(s / 60);
  if (m < 60) return m === 1 ? "1 minute" : `${m} minutes`;
  const h = Math.round(m / 60);
  return h === 1 ? "1 hour" : `${h} hours`;
}

/** The sync item in the status bar: this device's part, then the others. */
export function syncLabel(team: TeamStatus | null): string {
  if (!team || team.paired.length === 0) return "This device only";
  const p = team.presenter;
  if (p.role === "Presenting") {
    const synced = team.paired.filter((d) => d.connected).length;
    if (synced === 0) return "Presenting · other devices offline";
    return `Presenting · ${synced + 1} devices synced`;
  }
  switch (p.standing) {
    case "Open":
      return "Planning · nobody is presenting";
    case "Wait":
    case "TakeOver":
      return `Planning · ${p.holder_name} is offline`;
    default:
      return `Planning · ${p.holder_name} is presenting`;
  }
}

/** What the one button for taking a service live does on this device. */
export type PresentChoice =
  | { kind: "go" }
  | { kind: "claim"; label: string }
  | { kind: "ask"; label: string }
  | { kind: "takeover"; label: string; confirm: string }
  | { kind: "waiting"; label: string }
  | { kind: "declined"; label: string; again: string }
  | { kind: "wait"; label: string; hint: string };

/** `go` when this device presents; otherwise the way to become the presenter. */
export function presentChoice(p: PresenterView | null | undefined, present = "Present from here"): PresentChoice {
  if (!p || p.role === "Presenting") return { kind: "go" };
  const who = p.holder_name || "the presenting device";
  if (p.asking) {
    return p.asking.declined
      ? { kind: "declined", label: `${who} said not now`, again: "Ask again" }
      : { kind: "waiting", label: `Waiting for ${who} to allow…` };
  }
  switch (p.standing) {
    case "Open":
      return { kind: "claim", label: present };
    case "TakeOver":
      return {
        kind: "takeover",
        label: present,
        confirm: `${who} has been offline for ${howLong(p.offline_secs ?? 0)}. Present from here?`,
      };
    case "Wait":
      return {
        kind: "wait",
        label: present,
        hint: `${who} went offline ${howLong(p.offline_secs ?? 0)} ago. You can take over in ${howLong(p.wait_secs ?? 0)}.`,
      };
    default:
      return { kind: "ask", label: "Ask to present" };
  }
}

/** What the presenting device says when another device asks to present. */
export function requestPrompt(name: string, onAir: boolean): string {
  const who = name || "Another device";
  return onAir
    ? `${who} wants to present. Something is on air here: allowing hands over the screens and takes it off air.`
    : `${who} wants to present.`;
}

/** Said once on a device that was presenting when another took over. */
export function replacedNotice(by: string): string {
  return `${by} took over presenting while this device was offline or busy. This device is now planning: its screens are dark and it can't take a service live. You can ask to present again.`;
}

/** The takeover times offered in Setup › Devices, in seconds. */
export const TAKEOVER_CHOICES = [30, 60, 120, 300, 600, 1800] as const;

/** Setup › Screens: what happens to screens turned on while this device only plans. */
export const PLANNING_SCREENS =
  "This device is planning, so its screens stay dark. They open by themselves when this device presents.";
