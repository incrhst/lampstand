// Looping sections: where a section starts and ends, and how long one pass
// of it takes. A section is a header and the items after it, up to the next
// header; items before the first header are in no section.

import type { ItemEstimate } from "./api";

/** One row of a service's order, as far as sections care. */
export type Row = { id: string; header: boolean; title: string };

/** The header that owns row `index`, or -1 for a row before the first header. */
export function sectionOf(rows: Row[], index: number): number {
  for (let i = Math.min(index, rows.length - 1); i >= 0; i--) {
    if (rows[i].header) return i;
  }
  return -1;
}

/** The row after the section that `header` starts: the next header, or the end. */
export function sectionEnd(rows: Row[], header: number): number {
  for (let i = header + 1; i < rows.length; i++) {
    if (rows[i].header) return i;
  }
  return rows.length;
}

/** How many items sit under the header at `header`. */
export function sectionSize(rows: Row[], header: number): number {
  return sectionEnd(rows, header) - header - 1;
}

/** How many items would sit under a header put in front of row `at`. */
export function coveredAt(rows: Row[], at: number): number {
  let n = 0;
  for (let i = at; i < rows.length && !rows[i].header; i++) n++;
  return n;
}

/** One item's part of a pass. */
export type Line = {
  id: string;
  title: string;
  /** Still slides, each up for the interval. */
  stills: number;
  /** Of those, deck slides that build on click and show finished. */
  clickSlides: number;
  /** A video that plays through: its length. */
  videoMs: number | null;
  /** A music-only item: how long it plays. Counts like a video. */
  soundMs: number | null;
  /** A video that repeats itself, so the loop never moves on. */
  selfLoop: boolean;
  /** No fixed length (a countdown). */
  openEnded: boolean;
  /** The time this item takes in one pass; 0 when it has no fixed length. */
  ms: number;
};

export type PassEstimate = {
  lines: Line[];
  /** The part of a pass that has a length. */
  ms: number;
  /** Items with no fixed length, which make `ms` a minimum. */
  openEnded: string[];
  /** Videos that repeat themselves. */
  selfLoops: Line[];
  clickSlides: number;
};

/** One pass of the items in `rows`, with each still slide up for `seconds`. */
export function estimatePass(
  rows: Row[],
  estimates: Record<string, ItemEstimate | undefined>,
  seconds: number,
): PassEstimate {
  const lines: Line[] = [];
  for (const row of rows) {
    const e = estimates[row.id];
    if (!e) continue;
    const video = e.video_ms != null && !e.self_loop ? e.video_ms : 0;
    const sound = e.sound_ms ?? 0;
    const open = e.open_ended || e.self_loop;
    lines.push({
      id: row.id,
      title: row.title,
      stills: e.stills,
      clickSlides: e.click_slides,
      videoMs: e.video_ms,
      soundMs: e.sound_ms ?? null,
      selfLoop: e.self_loop,
      openEnded: e.open_ended,
      ms: open ? 0 : e.stills * seconds * 1000 + video + sound,
    });
  }
  return {
    lines,
    ms: lines.reduce((sum, l) => sum + l.ms, 0),
    openEnded: lines.filter((l) => l.openEnded || l.selfLoop).map((l) => l.title),
    selfLoops: lines.filter((l) => l.selfLoop),
    clickSlides: lines.reduce((sum, l) => sum + l.clickSlides, 0),
  };
}

/** 87 seconds is "1:27". */
export function formatLength(ms: number): string {
  const total = Math.round(ms / 1000);
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, "0")}`;
}

/** "≈ 1:27", or "At least 1:27" when something in the pass has no fixed length. */
export function passLabel(pass: PassEstimate): string {
  return pass.openEnded.length > 0 ? `At least ${formatLength(pass.ms)}` : `≈ ${formatLength(pass.ms)}`;
}

/** What one line of a pass says about itself. */
export function describeLine(line: Line, seconds: number): string {
  if (line.selfLoop) return "Repeats by itself, so the loop never moves on";
  if (line.openEnded) return "No fixed length";
  const parts: string[] = [];
  if (line.stills > 0) {
    parts.push(`${line.stills} ${line.stills === 1 ? "slide" : "slides"} × ${seconds} s`);
  }
  if (line.videoMs != null) parts.push(`plays through, ${formatLength(line.videoMs)}`);
  if (line.soundMs != null) parts.push(`music, ${formatLength(line.soundMs)}`);
  if (line.clickSlides > 0) {
    parts.push(`${line.clickSlides} with click-only builds, shown finished`);
  }
  return parts.join(" · ");
}
