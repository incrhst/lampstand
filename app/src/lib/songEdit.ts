// The song editor's working copy: text fields as typed, turned back into a
// `Song` on save. Lines are kept exactly as typed, so ChordPro markup like
// "[G]Amazing [G7]grace" survives untouched.

import type { Song } from "./api";

export interface SectionDraft {
  /** Keeps a section's inputs steady while others are added, removed or moved. */
  uid: number;
  label: string;
  /** One line per row, as typed. */
  text: string;
}

export interface SongDraft {
  title: string;
  author: string;
  copyright: string;
  ccli: string;
  key: string;
  tempo: string;
  sections: SectionDraft[];
  arrangement: string[];
}

let nextUid = 1;
export const newSection = (label = "", text = ""): SectionDraft => ({ uid: nextUid++, label, text });

export function toDraft(song: Song): SongDraft {
  return {
    title: song.title,
    author: song.author ?? "",
    copyright: song.copyright ?? "",
    ccli: song.ccli_number?.toString() ?? "",
    key: song.key ?? "",
    tempo: song.tempo?.toString() ?? "",
    sections: song.sections.map((s) => newSection(s.label, s.lines.join("\n"))),
    arrangement: [...song.arrangement],
  };
}

const optional = (text: string): string | null => (text.trim() === "" ? null : text.trim());

/** A whole number from 0 to `max`, null when blank, undefined when it is something else. */
function whole(text: string, max: number): number | null | undefined {
  const t = text.trim();
  if (t === "") return null;
  const n = Number(t);
  return /^\d+$/.test(t) && n <= max ? n : undefined;
}

/** What is wrong with a draft, in the words shown beside the Save button. */
export function problems(draft: SongDraft): string[] {
  const found: string[] = [];
  if (draft.title.trim() === "") found.push("The song needs a title");
  if (whole(draft.ccli, 0xffff_ffff) === undefined) found.push("The CCLI number should be digits only");
  if (whole(draft.tempo, 0xffff) === undefined) found.push("The tempo should be a number of beats per minute");
  if (draft.sections.some((s) => s.label.trim() === "")) found.push("Every section needs a label");
  return found;
}

/** The song to save: `original` with the draft's fields. Call only when `problems` is empty. */
export function toSong(original: Song, draft: SongDraft): Song {
  return {
    ...original,
    title: draft.title.trim(),
    author: optional(draft.author),
    copyright: optional(draft.copyright),
    ccli_number: whole(draft.ccli, 0xffff_ffff) ?? null,
    key: optional(draft.key),
    tempo: whole(draft.tempo, 0xffff) ?? null,
    sections: draft.sections.map((s) => ({ label: s.label.trim(), lines: lines(s.text) })),
    arrangement: draft.arrangement,
  };
}

/** The lines of a textarea, without the blank ones at the end. */
export function lines(text: string): string[] {
  const all = text.split(/\r?\n/);
  while (all.length > 0 && all[all.length - 1].trim() === "") all.pop();
  return all;
}

/** `list` with the item at `from` moved by `by` places (clamped to the ends). */
export function moved<T>(list: T[], from: number, by: number): T[] {
  const to = Math.min(Math.max(from + by, 0), list.length - 1);
  if (from < 0 || from >= list.length || to === from) return list;
  const copy = [...list];
  copy.splice(to, 0, ...copy.splice(from, 1));
  return copy;
}

/** A label for the next section added: "V1" for an empty song, then one past the highest verse. */
export function nextLabel(labels: string[]): string {
  const verses = labels.map((l) => /^v(\d+)$/i.exec(l.trim())).filter((m) => m !== null);
  return `V${verses.reduce((n, m) => Math.max(n, Number(m[1])), 0) + 1}`;
}
