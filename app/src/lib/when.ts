// When a service starts: how it reads in lists and headers, the coming
// Sunday a new service defaults to, and the date and time inputs.
//
// A start time is UTC milliseconds plus the IANA zone it was planned in
// (`Service.time_zone`). Everything here reads and writes wall-clock time
// in that zone, so "Sunday 10:00" stays Sunday 10:00 for a planner on a
// laptop set to another zone, and across daylight-saving changes. With no
// zone, the device's own zone is used.

import type { Timestamp } from "./api";

/** The hour a new service starts at unless the planner picks another. */
export const DEFAULT_SERVICE_HOUR = 10;

/** This device's IANA time zone, e.g. "Europe/London". */
export function deviceZone(): string {
  return Intl.DateTimeFormat().resolvedOptions().timeZone;
}

/** Every IANA zone this runtime knows, for a picker; always includes `keep`. */
export function timeZones(...keep: (string | null | undefined)[]): string[] {
  const all: string[] =
    typeof Intl.supportedValuesOf === "function" ? Intl.supportedValuesOf("timeZone") : [];
  const extra = keep.filter((z): z is string => !!z && !all.includes(z));
  return [...all, ...extra].sort();
}

/** Whether `zone` is a time zone this runtime can use. */
export function isZone(zone: string): boolean {
  try {
    new Intl.DateTimeFormat("en-US", { timeZone: zone });
    return true;
  } catch {
    return false;
  }
}

type Wall = { year: number; month: number; day: number; hour: number; minute: number; second: number };

const wallFormats = new Map<string, Intl.DateTimeFormat>();

/** The wall-clock date and time at instant `at` in `zone`. */
function wallClock(at: number, zone: string): Wall {
  let format = wallFormats.get(zone);
  if (!format) {
    format = new Intl.DateTimeFormat("en-US", {
      timeZone: zone,
      hourCycle: "h23",
      year: "numeric",
      month: "numeric",
      day: "numeric",
      hour: "numeric",
      minute: "numeric",
      second: "numeric",
    });
    wallFormats.set(zone, format);
  }
  const parts: Record<string, number> = {};
  for (const p of format.formatToParts(at)) if (p.type !== "literal") parts[p.type] = Number(p.value);
  return {
    year: parts.year,
    month: parts.month,
    day: parts.day,
    hour: parts.hour,
    minute: parts.minute,
    second: parts.second,
  };
}

/** How far `zone` is ahead of UTC at instant `at`, in milliseconds. */
function offsetAt(at: number, zone: string): number {
  const w = wallClock(at, zone);
  const asUtc = Date.UTC(w.year, w.month - 1, w.day, w.hour, w.minute, w.second);
  return asUtc - Math.floor(at / 1000) * 1000;
}

/**
 * The instant a wall-clock time in `zone` happens. A time skipped by a
 * daylight-saving change moves forward by the gap; a repeated one picks
 * the first.
 */
export function zonedTime(
  year: number,
  month: number,
  day: number,
  hour: number,
  minute: number,
  zone: string,
): Timestamp {
  const asUtc = Date.UTC(year, month - 1, day, hour, minute);
  // The offset before and after the guess: they differ only near a change.
  const early = offsetAt(asUtc - 12 * 3_600_000, zone);
  const late = offsetAt(asUtc + 12 * 3_600_000, zone);
  const first = asUtc - early;
  if (early === late) return first;
  // Use the earlier reading if it lands on the asked wall time.
  const w = wallClock(first, zone);
  if (w.hour === hour && w.minute === minute) return first;
  const second = asUtc - late;
  const w2 = wallClock(second, zone);
  if (w2.hour === hour && w2.minute === minute) return second;
  // In a gap: read with the earlier offset, which lands past the change
  // by as much as the asked time is past its start.
  return first;
}

/**
 * The coming Sunday at 10:00 in `zone`: today if it is Sunday there and
 * not yet 10:00, otherwise the next Sunday. Where a new service is
 * scheduled.
 */
export function comingSunday(
  now: Date | number = Date.now(),
  zone: string = deviceZone(),
  hour = DEFAULT_SERVICE_HOUR,
): Timestamp {
  const at = typeof now === "number" ? now : now.getTime();
  const today = wallClock(at, zone);
  const weekday = new Date(Date.UTC(today.year, today.month - 1, today.day)).getUTCDay();
  const sunday = (days: number) => {
    const d = new Date(Date.UTC(today.year, today.month - 1, today.day + days));
    return zonedTime(d.getUTCFullYear(), d.getUTCMonth() + 1, d.getUTCDate(), hour, 0, zone);
  };
  const days = (7 - weekday) % 7;
  const next = sunday(days);
  return days === 0 && at >= next ? sunday(7) : next;
}

const pad = (n: number) => String(n).padStart(2, "0");

/** A start time as `<input type="date">` and `type="time"` values in `zone`. */
export function toInputs(at: Timestamp, zone: string = deviceZone()): { date: string; time: string } {
  const w = wallClock(at, zone);
  return { date: `${w.year}-${pad(w.month)}-${pad(w.day)}`, time: `${pad(w.hour)}:${pad(w.minute)}` };
}

/**
 * The start time for a date ("2026-10-25") and time ("10:00") in `zone`,
 * or null without a valid date. A missing time means the default hour.
 */
export function fromInputs(date: string, time: string, zone: string = deviceZone()): Timestamp | null {
  const d = /^(\d{4})-(\d{2})-(\d{2})$/.exec(date);
  if (!d) return null;
  const t = /^(\d{1,2}):(\d{2})$/.exec(time);
  const [hour, minute] = t ? [Number(t[1]), Number(t[2])] : [DEFAULT_SERVICE_HOUR, 0];
  const [year, month, day] = [Number(d[1]), Number(d[2]), Number(d[3])];
  if (month < 1 || month > 12 || day < 1 || day > 31 || hour > 23 || minute > 59) return null;
  return zonedTime(year, month, day, hour, minute, zone);
}

/**
 * "Sun 25 Oct · 10:30" in the service's zone, or "No date set". When that
 * zone's clock differs from the viewer's at that moment, the zone is named
 * ("Sun 25 Oct · 10:30 GMT-5") so nobody mistakes it for their own time.
 */
export function formatWhen(
  startsAt: Timestamp | null | undefined,
  zone?: string | null,
  locale?: string,
  viewerZone: string = deviceZone(),
): string {
  if (startsAt == null) return "No date set";
  const timeZone = zone && isZone(zone) ? zone : viewerZone;
  const day = new Date(startsAt).toLocaleDateString(locale, {
    timeZone,
    weekday: "short",
    day: "numeric",
    month: "short",
  });
  const time = new Date(startsAt).toLocaleTimeString(locale, { timeZone, hour: "2-digit", minute: "2-digit" });
  const text = `${day} · ${time}`;
  if (offsetAt(startsAt, timeZone) === offsetAt(startsAt, viewerZone)) return text;
  return `${text} ${zoneName(startsAt, timeZone, locale)}`;
}

/** The short name of `zone` at `at`: "BST", "EST", or "GMT-5" where none. */
export function zoneName(at: Timestamp, zone: string, locale?: string): string {
  const parts = new Intl.DateTimeFormat(locale, { timeZone: zone, timeZoneName: "short" }).formatToParts(at);
  return parts.find((p) => p.type === "timeZoneName")?.value ?? zone;
}
