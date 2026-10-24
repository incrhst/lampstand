// Roles, in words (docs/roles.md). The core decides who may take what
// (crates/lampstand-sync, roles.rs); this turns what it reports into the
// prompts, buttons and notices the screens show. Every string a volunteer
// reads about roles is in this file, so it can be reviewed in one place; the
// pull request for it lists each one with its source in the spec.

import type { RoleName, RoleView, TeamStatus } from "./api";

export const ROLES: RoleName[] = ["Projection", "Operator", "Stream", "Audio"];

/** What each role runs, as the spec's roster says it: "Runs the screens". */
export function job(role: RoleName): string {
  switch (role) {
    case "Projection":
      return "the screens";
    case "Operator":
      return "the service";
    case "Stream":
      return "the stream";
    case "Audio":
      return "the sound";
  }
}

/** A role's row in the roster: "Runs the stream". */
export function runs(role: RoleName): string {
  return `Runs ${job(role)}`;
}

/** The four roles as this device last heard them; empty on an old or missing status. */
export function rolesOf(team: TeamStatus | null | undefined): RoleView[] {
  return team?.roles ?? [];
}

export function roleOf(team: TeamStatus | null | undefined, role: RoleName): RoleView | null {
  return rolesOf(team).find((r) => r.role === role) ?? null;
}

/** "30 seconds", "1 minute", "5 minutes", "2 hours". */
export function howLong(seconds: number): string {
  const s = Math.max(0, Math.round(seconds));
  if (s < 60) return s === 1 ? "1 second" : `${s} seconds`;
  const m = Math.round(s / 60);
  if (m < 60) return m === 1 ? "1 minute" : `${m} minutes`;
  const h = Math.round(m / 60);
  return h === 1 ? "1 hour" : `${h} hours`;
}

// ---- The holder's prompt --------------------------------------------------

/** What the holder reads when a device asks: "Mary's laptop wants to run the stream." */
export function requestPrompt(role: RoleName, name: string): string {
  const who = name || "Another device";
  const line = `${who} wants to run ${job(role)}.`;
  switch (role) {
    case "Projection":
      return `${line} Allowing clears the screens.`;
    case "Audio":
      return `${line} Allowing stops the music.`;
    default:
      return line;
  }
}

/** The Allow button: it says what passing the role takes off air. */
export function allowLabel(role: RoleName): string {
  switch (role) {
    case "Projection":
      return "Allow and clear the screens";
    case "Audio":
      return "Allow and stop the music";
    default:
      return "Allow";
  }
}

export const NOT_NOW = "Not now";
export const ALWAYS_ALLOW = "Always allow this device";

// ---- The device that lost a role -----------------------------------------

/** Said once on a device that held the role while it was away. */
export function replacedNotice(role: RoleName, by: string): string {
  const who = by || "Another device";
  return `${who} took over running ${job(role)} while this device was offline. This device no longer runs ${job(role)}.`;
}

// ---- The asker's button ---------------------------------------------------

/** Why the button is greyed out, in the spec's words where it has them. */
export const NO_SCREENS = "No screens are set up on this device";
export const NOBODY_RUNS_SCREENS = "The service can't be run until someone runs the screens.";

export type RoleChoice =
  | { kind: "none" }
  | { kind: "claim"; label: string }
  | { kind: "ask"; label: string }
  | { kind: "takeover"; label: string; confirm: string; ok: string }
  | { kind: "waiting"; label: string }
  | { kind: "declined"; label: string; again: string }
  | { kind: "busy"; label: string; again: string }
  | { kind: "blocked"; label: string; hint: string }
  | { kind: "wait"; label: string; hint: string };

/** The spec's button. */
export const TAKE = "Take";

/**
 * What the one "Take" button for a role does on this device: nothing where
 * this device already holds it, a click where nobody does, an ask where the
 * holder is connected, a confirmed takeover where it has been away long
 * enough, and a greyed button that says why otherwise.
 */
export function roleChoice(r: RoleView | null | undefined): RoleChoice {
  if (!r || r.holds || r.standing === "Holding") return { kind: "none" };
  const who = r.holder_name || `The device that runs ${job(r.role)}`;
  const a = r.asking;
  // An answer that came back stays until it is dismissed. An ask nobody
  // has answered stops mattering once the holder is offline: no answer can
  // come, so the way to take over shows instead.
  if (a === "Declined") return { kind: "declined", label: `${who} said not now.`, again: "Ask again" };
  if (a && typeof a === "object") return { kind: "busy", label: `${a.Busy.by} is already asking.`, again: "Ask again" };
  if (a === "Waiting" && r.standing === "Ask") return { kind: "waiting", label: `Waiting for ${who} to allow…` };
  if (r.blocked === "NoScreens") return { kind: "blocked", label: TAKE, hint: NO_SCREENS };
  if (r.blocked === "NeedsProjection") return { kind: "blocked", label: TAKE, hint: NOBODY_RUNS_SCREENS };
  switch (r.standing) {
    case "Open":
      return { kind: "claim", label: TAKE };
    case "TakeOver":
      return {
        kind: "takeover",
        label: TAKE,
        confirm: `${who} has been offline for ${howLong(r.offline_secs ?? 0)}. Run ${job(r.role)} from here?`,
        ok: TAKE,
      };
    case "Wait":
      return {
        kind: "wait",
        label: TAKE,
        hint: `${who} went offline ${howLong(r.offline_secs ?? 0)} ago. You can take over in ${howLong(r.wait_secs ?? 0)}.`,
      };
    default:
      return { kind: "ask", label: TAKE };
  }
}

/** The button's accessible name: its visible word plus what it takes. */
export function takeName(role: RoleName, visible: string = TAKE): string {
  return visible === TAKE ? `${TAKE} ${job(role)}` : visible;
}

/** The Take button's label where it stands alone: "Run the service from here". */
export function runFromHere(role: RoleName): string {
  return `Run ${job(role)} from here`;
}

/**
 * Allow is not offered for the Operator while nobody runs the screens: the
 * core would refuse. Says why instead.
 */
export function allowBlocked(team: TeamStatus | null | undefined, role: RoleName): string | null {
  if (role !== "Operator") return null;
  const p = roleOf(team, "Projection");
  return p && !p.holds && !p.holder ? NOBODY_RUNS_SCREENS : null;
}

/** Who has it, for a role's row. */
export function holderLine(r: RoleView): string {
  if (r.holds) return "This device";
  return r.holder_name || (r.holder ? "Another device" : "Nobody");
}

const ASK_STATES: string[] = ["waiting", "declined", "busy"];

/** Roles whose prompt or notice should show across the top of the screens. */
export function wantsAttention(team: TeamStatus | null | undefined): boolean {
  return rolesOf(team).some((r) => r.request || r.replaced_by || ASK_STATES.includes(roleChoice(r).kind));
}

/** The panel's heading, from the spec's page title ("Roles: who runs what"). */
export const PANEL_TITLE = "Who runs what";

/** On the holder: a device it hands the role to without asking. */
export const ALWAYS_ALLOWED = "Always allowed";
export function stopAlways(name: string): string {
  return `Stop always allowing ${name}`;
}

/**
 * The Operator seat is with the device that runs the screens: the driver's
 * own "took control back" notice says that, so the generic one stays quiet.
 * Compared by device key, not by name.
 */
export function operatorTakenBack(team: TeamStatus | null | undefined): boolean {
  const op = roleOf(team, "Operator");
  const projection = roleOf(team, "Projection");
  return !!op?.replaced_by && !!op.holder && op.holder === projection?.holder;
}

/** The roles whose "you lost it" notice the generic banner shows. */
export function noticeRoles(team: TeamStatus | null | undefined): RoleView[] {
  return rolesOf(team).filter((r) => r.replaced_by && !(r.role === "Operator" && operatorTakenBack(team)));
}
