// How a service's start time reads in lists and headers.

import type { Timestamp } from "./api";

/** "Sun 19 Oct · 10:30", or "No date set" for a service without one. */
export function formatWhen(startsAt: Timestamp | null | undefined, locale?: string): string {
  if (startsAt == null) return "No date set";
  const date = new Date(startsAt);
  const day = date.toLocaleDateString(locale, { weekday: "short", day: "numeric", month: "short" });
  const time = date.toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit" });
  return `${day} · ${time}`;
}

/** The hour a new service starts at unless the planner picks another. */
export const DEFAULT_SERVICE_HOUR = 10;

/**
 * The coming Sunday at 10:00 local time: today if it is Sunday and not yet
 * 10:00, otherwise the next Sunday. Where a new service is scheduled.
 */
export function comingSunday(now: Date = new Date(), hour = DEFAULT_SERVICE_HOUR): Date {
  const at = new Date(now.getFullYear(), now.getMonth(), now.getDate(), hour, 0, 0, 0);
  let days = (7 - now.getDay()) % 7;
  if (days === 0 && now.getTime() >= at.getTime()) days = 7;
  at.setDate(at.getDate() + days);
  return at;
}

const pad = (n: number) => String(n).padStart(2, "0");

/** A local date and time as `<input type="date">` and `type="time"` values. */
export function toInputs(at: Date | Timestamp): { date: string; time: string } {
  const d = new Date(at);
  return {
    date: `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`,
    time: `${pad(d.getHours())}:${pad(d.getMinutes())}`,
  };
}

/**
 * The timestamp for a local date ("2026-10-25") and time ("10:00"), or
 * null without a valid date. A missing time means the default hour.
 */
export function fromInputs(date: string, time: string): Timestamp | null {
  const d = /^(\d{4})-(\d{2})-(\d{2})$/.exec(date);
  if (!d) return null;
  const t = /^(\d{1,2}):(\d{2})$/.exec(time);
  const [hour, minute] = t ? [Number(t[1]), Number(t[2])] : [DEFAULT_SERVICE_HOUR, 0];
  const at = new Date(Number(d[1]), Number(d[2]) - 1, Number(d[3]), hour, minute);
  return Number.isNaN(at.getTime()) ? null : at.getTime();
}
