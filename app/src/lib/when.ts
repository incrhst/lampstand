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
