// The planner's library panel without the panel: how one search ranks
// songs, scripture, decks and media together, how dropped files are
// sorted to the request that imports them, and the short lines it shows
// afterwards. Kept free of Svelte and Tauri so node can test it.

import type { ItemKind, SongHit } from "./api";
import { deckExtensions, imageExtensions, videoExtensions } from "./media.ts";
import { audioExtensions, linkOffers, streamingRefusal } from "./sound.ts";

export type Kind = "song" | "scripture" | "deck" | "media";
export type Filter = "all" | Kind;

/** One row of the library: anything that can go in a service. */
export interface Entry {
  /** Unique across kinds: "song:<id>", "deck:<id>", "media:<blob>"... */
  key: string;
  kind: Kind;
  title: string;
  subtitle: string;
  /** The lyric line a search matched, shown in place of the subtitle. */
  snippet?: string;
  /** What adding it inserts; null when it cannot be added (a failed lookup). */
  item: ItemKind | null;
  /** For a deck: which version of its file this is, so its thumbnail is drawn again when it changes. */
  version?: string;
  /** Added to the library and not yet put in a service: marked "New" in Recent. */
  fresh?: boolean;
}

const terms = (query: string) => query.toLowerCase().split(/\s+/).filter(Boolean);

/**
 * How well a title matches: 3 starts with the query, 2 contains it, 1 has
 * every word of it somewhere, 0 no match.
 */
export function titleScore(title: string, query: string): number {
  const q = query.trim().toLowerCase();
  if (!q) return 0;
  const t = title.toLowerCase();
  if (t.startsWith(q)) return 3;
  if (t.includes(q)) return 2;
  return terms(q).every((w) => t.includes(w)) ? 1 : 0;
}

const byTitle = (a: Entry, b: Entry) => a.title.localeCompare(b.title, undefined, { sensitivity: "base" });

/** Every entry, by title (the library with no search). */
export function sortByTitle(entries: Entry[]): Entry[] {
  return [...entries].sort(byTitle);
}

/**
 * One result list for a search. `passage` (a reference the query reads
 * as) goes first; then songs, decks and media by how well their title
 * matches, keeping the core's order for songs that tie. Decks and media
 * are matched here by title; songs come from the core's search and keep
 * their lyric hits below the title hits.
 */
export function rank(query: string, passage: Entry | null, songs: Entry[], others: Entry[]): Entry[] {
  const scored = [
    ...songs.map((entry) => ({ entry, score: titleScore(entry.title, query) || 0.5 })),
    ...others
      .map((entry) => ({ entry, score: titleScore(entry.title, query) }))
      .filter((s) => s.score > 0)
      .sort((a, b) => b.score - a.score || byTitle(a.entry, b.entry)),
  ];
  // Array.prototype.sort is stable, so ties keep songs first, in order.
  const ranked = scored.sort((a, b) => b.score - a.score).map((s) => s.entry);
  return passage ? [passage, ...ranked.filter((e) => e.key !== passage.key)] : ranked;
}

/** How many of each kind a result list has, for the filter chips. */
export function countKinds(entries: Entry[]): Record<Filter, number> {
  const counts: Record<Filter, number> = { all: entries.length, song: 0, scripture: 0, deck: 0, media: 0 };
  for (const e of entries) counts[e.kind]++;
  return counts;
}

export function byFilter(entries: Entry[], filter: Filter): Entry[] {
  return filter === "all" ? entries : entries.filter((e) => e.kind === filter);
}

/**
 * The Recent list: what was put in a service or just added to the library,
 * newest first, once each, at most `max`. Putting a row in a service later
 * replaces its "New" mark.
 */
export function remember(recent: Entry[], entry: Entry, max = 8): Entry[] {
  return [entry, ...recent.filter((e) => e.key !== entry.key)].slice(0, max);
}

/**
 * The songs in `after` that were not in the library before an import, when
 * there are few enough to be worth listing. A bulk import (a whole songbook)
 * is not "recent": it would bury everything else.
 */
export function newSongs(before: Iterable<string>, after: SongHit[], max = 6): SongHit[] {
  const known = new Set(before);
  const added = after.filter((h) => !known.has(h.song));
  return added.length <= max ? added : [];
}

/** A pasted share link, which adds a deck rather than searching. */
export function isLink(query: string): boolean {
  return /^https?:\/\/\S+$/i.test(query.trim());
}

/**
 * The rows a pasted link offers: a deck, a sound, a song from a web page,
 * or (for a streaming page) the reason it can't be added. A Drive or
 * OneDrive link could be a deck or a sound, so it offers both.
 */
export function linkEntries(link: string): Entry[] {
  const url = link.trim();
  const offers = linkOffers(url);
  const rows: Entry[] = [];
  if (offers.deck) rows.push({ key: "link", kind: "deck", title: "Add deck from link", subtitle: url, item: null });
  if (offers.sound) rows.push({ key: "link:sound", kind: "media", title: "Add sound from link", subtitle: url, item: null });
  if (offers.song) rows.push({ key: "link:song", kind: "song", title: "Add song from link", subtitle: url, item: null });
  if (offers.streaming) {
    rows.push({ key: "link:streaming", kind: "media", title: "This link can't be added", subtitle: streamingRefusal, item: null });
  }
  return rows;
}

/** Whether a search should be read as a Bible reference: it has a number in it. */
export function mightBeReference(query: string): boolean {
  return /\d/.test(query);
}

/** Dropped or picked paths, by the request that takes them. */
export interface Sorted {
  /** Songs, Bibles and folders, for `ImportFiles`, which sniffs them. */
  imports: string[];
  /** PowerPoint files, for `AddDeckFile`. */
  decks: string[];
  /** Images and video, for `AddMediaFile`. */
  media: string[];
  /** Music and other sound, for `AddSoundFile`, then the open service. */
  sounds: string[];
}

function extension(path: string): string {
  const name = path.split(/[\\/]/).pop() ?? "";
  const dot = name.lastIndexOf(".");
  return dot <= 0 ? "" : name.slice(dot + 1).toLowerCase();
}

/**
 * Sorts paths by extension. Anything that is not a deck, image or video
 * (a folder, an .osb, an OpenSong file with no extension) goes to
 * `ImportFiles`, which works out the format and reports what it skips.
 * Hidden files (".DS_Store") are dropped.
 */
export function sortPaths(paths: string[]): Sorted {
  const sorted: Sorted = { imports: [], decks: [], media: [], sounds: [] };
  for (const path of paths) {
    const name = path.split(/[\\/]/).pop() ?? "";
    if (!name || name.startsWith(".")) continue;
    const ext = extension(path);
    if (deckExtensions.includes(ext)) sorted.decks.push(path);
    else if (videoExtensions.includes(ext) || imageExtensions.includes(ext)) sorted.media.push(path);
    else if (audioExtensions.includes(ext)) sorted.sounds.push(path);
    else sorted.imports.push(path);
  }
  return sorted;
}

/** Songs and Bibles from `ImportFiles`' summary line ("Imported 3 songs. 1 updated. Bibles: KJV"). */
export function importedCounts(message: string): { songs: number; bibles: number; updated: number } {
  const songs = Number(/Imported (\d+) songs?/.exec(message)?.[1] ?? 0);
  const updated = Number(/(\d+) updated/.exec(message)?.[1] ?? 0);
  const list = /Bibles: ([^.]*)/.exec(message)?.[1];
  return { songs, bibles: list ? list.split(", ").filter(Boolean).length : 0, updated };
}

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

/** "3 songs · 1 deck", or "Nothing new to add" when every count is zero. */
export function summarize(added: { songs?: number; updated?: number; bibles?: number; decks?: number; media?: number; sounds?: number }): string {
  const parts = [
    added.songs ? plural(added.songs, "song", "songs") : "",
    added.updated ? `${plural(added.updated, "song", "songs")} updated` : "",
    added.bibles ? plural(added.bibles, "Bible", "Bibles") : "",
    added.decks ? plural(added.decks, "deck", "decks") : "",
    added.media ? plural(added.media, "media file", "media files") : "",
    added.sounds ? plural(added.sounds, "sound", "sounds") : "",
  ].filter(Boolean);
  return parts.length ? parts.join(" · ") : "Nothing new to add";
}

/** How far a song import is, from its progress message ("Adding songs: 300 of 3100"). */
export function songProgress(message: string): number | null {
  const m = /Adding songs: (\d+) of (\d+)/.exec(message);
  return m && Number(m[2]) > 0 ? Number(m[1]) / Number(m[2]) : null;
}
