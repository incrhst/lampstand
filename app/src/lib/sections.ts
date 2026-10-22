// Jumping to a song section by key: 1 to 9 for verses, C chorus, B bridge,
// P pre-chorus, T tag, I intro, E ending. Labels are read the way
// `SectionKind::from_label` reads them in lampstand-core ("V2", "Verse 2",
// "Chorus", "pre-chorus"), so keep the two in step.

import type { Cursor, LoadedService } from "./api";

type Kind = "verse" | "chorus" | "prechorus" | "bridge" | "tag" | "intro" | "ending" | "other";

const WORDS: Record<string, Kind> = {
  v: "verse",
  verse: "verse",
  c: "chorus",
  chorus: "chorus",
  refrain: "chorus",
  p: "prechorus",
  prechorus: "prechorus",
  b: "bridge",
  bridge: "bridge",
  t: "tag",
  tag: "tag",
  i: "intro",
  intro: "intro",
  e: "ending",
  ending: "ending",
  outro: "ending",
};

/** A label's kind and number: "V2" is verse 2, "Chorus" is chorus with no number. */
export function parseLabel(label: string): { kind: Kind; number: number | null } {
  const lower = label.trim().toLowerCase();
  const digits = lower.match(/(\d+)\s*$/);
  const word = lower.replace(/[\d\s]+$/, "").replace(/[\s_-]/g, "");
  return { kind: WORDS[word] ?? "other", number: digits ? Number(digits[1]) : null };
}

/** The section a key asks for, or null if the key is not a section key. */
export function keyTarget(key: string): { kind: Kind; number: number | null } | null {
  if (/^[1-9]$/.test(key)) return { kind: "verse", number: Number(key) };
  const letter: Record<string, Kind> = {
    c: "chorus",
    b: "bridge",
    p: "prechorus",
    t: "tag",
    i: "intro",
    e: "ending",
  };
  const kind = letter[key.toLowerCase()];
  return kind ? { kind, number: null } : null;
}

/**
 * Where a section key goes: the first slide of the next matching section
 * in the live item (wrapping round), so pressing C again after the chorus
 * finds the next chorus. With nothing live, the first item that has slides.
 * A verse key matches that verse's number; a letter matches every section
 * of its kind.
 */
export function sectionCursor(loaded: LoadedService, live: Cursor | null, key: string): Cursor | null {
  const want = keyTarget(key);
  if (!want) return null;
  const item =
    loaded.items.find((i) => i.id === live?.item && i.slides.length > 0) ??
    loaded.items.find((i) => i.slides.length > 0);
  if (!item) return null;
  const starts: number[] = [];
  item.slides.forEach((slide, index) => {
    // A section split over several slides repeats its label; a repeat of
    // the section later in the arrangement starts again after another one.
    const label = slide.label ?? "";
    if (index > 0 && (item.slides[index - 1].label ?? "") === label) return;
    const { kind, number } = parseLabel(label);
    if (kind !== want.kind) return;
    if (want.number !== null && (number ?? 1) !== want.number) return;
    starts.push(index);
  });
  if (starts.length === 0) return null;
  const current = live?.item === item.id ? live.slide : -1;
  const slide = starts.find((s) => s > current) ?? starts[0];
  return { item: item.id, slide, step: 0 };
}

/**
 * A typed order ("v1 v1, c v2") as labels: split on spaces and commas, case
 * kept. Reads the same as `parse_order` in lampstand-core.
 */
export function parseOrder(text: string): string[] {
  return text.split(/[\s,]+/).filter((l) => l !== "");
}

/** The labels in `order` that name none of `sections`, ignoring case. */
export function unknownLabels(order: string[], sections: string[]): string[] {
  const have = new Set(sections.map((s) => s.trim().toLowerCase()));
  return order.filter((l) => !have.has(l.trim().toLowerCase()));
}
