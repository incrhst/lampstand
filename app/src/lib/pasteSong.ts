// Pasting lyrics: the words around the paste dialog and what it does with
// what the core read. The reading itself is in the core
// (`LibraryRequest::ParseLyrics`); this file only turns its answer into what
// the dialog shows and saves.
//
// The words marked "proposed" are not in the spec and wait for the owner.

import type { ParsedSong, Song } from "./api";

export const words = {
  /** proposed: the button in the library. */
  button: "Paste lyrics",
  buttonTitle: "Paste the words of a song from a web page, an email or a chart",
  title: "Paste lyrics",
  /** proposed: above the box. */
  pasteLabel: "Paste the words here",
  pasteHint:
    "Headings such as Verse 1, Chorus or Bridge start a section. Chords above the words are left out. Check the result before you add it.",
  titleLabel: "Title",
  titleFromText: (title: string) => `Using “${title}” from the first line.`,
  moreDetails: "Author, copyright and CCLI number",
  authorLabel: "Author",
  copyrightLabel: "Copyright",
  ccliLabel: "CCLI number",
  keyLabel: "Key",
  /** proposed: shown over the preview before there is anything. */
  empty: "Nothing to show yet.",
  previewTitle: "How it will be saved",
  sung: (order: string[]) => `Sung in this order: ${order.join(", ")}`,
  lineCount: (n: number) => (n === 1 ? "1 line" : `${n} lines`),
  /** proposed: said when a song with this title is already in the library. */
  sameTitle: (title: string) =>
    `A song called “${title}” is already in the library. Adding this will make a second one.`,
  /** proposed: the licence reminder, shown once beside the buttons. */
  licence: "Only add words your church has the right to use.",
  cancel: "Cancel",
  addToLibrary: "Add to library",
  addToService: (title: string) => `Add to library and to “${title}”`,
  added: (title: string) => `Added ${title}`,
  addedToService: (title: string, service: string) => `Added ${title} to ${service}`,
};

/** The details the person types beside the words. */
export interface Details {
  title: string;
  author: string;
  copyright: string;
  ccli: string;
  key: string;
}

export const blankDetails = (): Details => ({ title: "", author: "", copyright: "", ccli: "", key: "" });

const optional = (text: string): string | null => (text.trim() === "" ? null : text.trim());

/** The title to use: what was typed, else what the paste had on its first line. */
export function effectiveTitle(typed: string, parsed: ParsedSong | null): string {
  return typed.trim() !== "" ? typed.trim() : (parsed?.song.title.trim() ?? "");
}

/** Whether the title is the paste's own guess, so the dialog can say so. */
export function titleFromText(typed: string, parsed: ParsedSong | null): string | null {
  const t = parsed?.song.title.trim() ?? "";
  return typed.trim() === "" && t !== "" ? t : null;
}

/** Why the song cannot be added yet, in words for the person; null when it can. */
export function problem(details: Details, parsed: ParsedSong | null, readError: string | null): string | null {
  if (readError) return readError;
  if (!parsed) return null;
  if (effectiveTitle(details.title, parsed) === "") return "The song needs a title.";
  if (details.ccli.trim() !== "" && !/^\d{1,10}$/.test(details.ccli.trim())) return "The CCLI number should be digits only.";
  if (details.ccli.trim() !== "" && Number(details.ccli.trim()) > 0xffff_ffff) return "The CCLI number is too long.";
  return null;
}

/** The song to save: what the core read, with the details typed beside it. */
export function toSong(parsed: ParsedSong, details: Details): Song {
  const ccli = details.ccli.trim();
  return {
    ...parsed.song,
    title: effectiveTitle(details.title, parsed),
    author: optional(details.author) ?? parsed.song.author,
    copyright: optional(details.copyright) ?? parsed.song.copyright,
    ccli_number: ccli === "" ? parsed.song.ccli_number : Number(ccli),
    key: optional(details.key) ?? parsed.song.key,
  };
}

/** One line per section for the preview: "V1 · 4 lines". */
export function previewRows(parsed: ParsedSong | null): { label: string; detail: string; first: string }[] {
  return (parsed?.song.sections ?? []).map((s) => ({
    label: s.label,
    detail: words.lineCount(s.lines.length),
    first: s.lines[0] ?? "",
  }));
}

/** The order the song is sung in when it is not the order written, else null. */
export function sungOrder(parsed: ParsedSong | null): string[] | null {
  return parsed && parsed.song.arrangement.length > 0 ? parsed.song.arrangement : null;
}
