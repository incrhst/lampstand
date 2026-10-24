// Who may present, in words. The core decides (crates/lampstand-sync,
// presenter.rs); this turns what it reports into the labels, prompts and
// choices the screens show.

import type { PresenterView, TeamStatus } from "./api";
import type { SyncPill } from "./sync";
import { drivenBy, drivingFrom } from "./driving.ts";

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

/**
 * The status bar's sync pill with this device's part in it: "Presenting ·
 * Synced with 2 devices", "Planning · Mary's laptop is presenting". When
 * sync has a problem the problem keeps the words.
 */
export function withRole(pill: SyncPill, team: TeamStatus | null): SyncPill {
  if (!team) return pill;
  if (team.paired.length === 0) {
    return {
      ...pill,
      text: `Presenting · ${pill.text}`,
      title: `${pill.title}. This device is the presenter. Once a second device is paired, a device that wants to present has to ask first`,
    };
  }
  const p = team.presenter;
  const by = drivenBy(team);
  if (p.role === "Presenting") return { ...pill, text: `Presenting${by ? ` · driven from ${by}` : ""} · ${pill.text}` };
  const driving = drivingFrom(team);
  if (driving) {
    const text = driving.online ? `Driving ${driving.name} · ${pill.text}` : `Driving ${driving.name} · offline, controls paused`;
    return { ...pill, tone: driving.online ? pill.tone : "warn", text, title: `${pill.title}. This device operates ${driving.name}; the screens stay there` };
  }
  if (pill.tone === "warn") return { ...pill, text: `Planning · ${pill.text}` };
  const who = p.holder_name || "another device";
  let text: string;
  switch (p.standing) {
    case "Open":
      text = "nobody is presenting";
      break;
    case "Wait":
    case "TakeOver":
      text = `${who} is offline`;
      break;
    default:
      text = `${who} is presenting`;
  }
  return { ...pill, text: `Planning · ${text}`, title: `${pill.title}. This device is planning: it can't take a service live` };
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
      return { kind: "ask", label: "Make this device the presenter" };
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
