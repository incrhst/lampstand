// Passages as people read them, for service rows. Mirrors
// `impl Display for Passage` in lampstand-core's scripture.rs.

import type { Passage } from "./api/index";

export const BOOK_NAMES = [
  "Genesis", "Exodus", "Leviticus", "Numbers", "Deuteronomy", "Joshua", "Judges", "Ruth",
  "1 Samuel", "2 Samuel", "1 Kings", "2 Kings", "1 Chronicles", "2 Chronicles", "Ezra",
  "Nehemiah", "Esther", "Job", "Psalms", "Proverbs", "Ecclesiastes", "Song of Solomon",
  "Isaiah", "Jeremiah", "Lamentations", "Ezekiel", "Daniel", "Hosea", "Joel", "Amos",
  "Obadiah", "Jonah", "Micah", "Nahum", "Habakkuk", "Zephaniah", "Haggai", "Zechariah",
  "Malachi", "Matthew", "Mark", "Luke", "John", "Acts", "Romans", "1 Corinthians",
  "2 Corinthians", "Galatians", "Ephesians", "Philippians", "Colossians", "1 Thessalonians",
  "2 Thessalonians", "1 Timothy", "2 Timothy", "Titus", "Philemon", "Hebrews", "James",
  "1 Peter", "2 Peter", "1 John", "2 John", "3 John", "Jude", "Revelation",
] as const;

/** `ChapterVerse::END_OF_CHAPTER`: the chapter's last verse. */
export const END_OF_CHAPTER = 0xffff;

/** Obadiah, Philemon, 2 John, 3 John and Jude. */
const SINGLE_CHAPTER = new Set([31, 57, 63, 64, 65]);

/** "John 3:16", "John 3:16–18", "Genesis 1:1–2:3", "Psalms 23", "Jude 3". */
export function formatPassage({ book, start: s, end: e }: Passage): string {
  const name = BOOK_NAMES[book - 1] ?? `Book ${book}`;
  const verse = (v: number) => (v === END_OF_CHAPTER ? "end" : String(v));
  if (SINGLE_CHAPTER.has(book) && s.chapter === 1 && e.chapter === 1) {
    if (s.verse === 1 && e.verse === END_OF_CHAPTER) return name;
    if (s.verse === e.verse) return `${name} ${s.verse}`;
    return `${name} ${s.verse}–${verse(e.verse)}`;
  }
  if (s.chapter === e.chapter) {
    if (s.verse === 1 && e.verse === END_OF_CHAPTER) return `${name} ${s.chapter}`;
    if (s.verse === e.verse) return `${name} ${s.chapter}:${s.verse}`;
    return `${name} ${s.chapter}:${s.verse}–${verse(e.verse)}`;
  }
  if (s.verse === 1 && e.verse === END_OF_CHAPTER) return `${name} ${s.chapter}–${e.chapter}`;
  return `${name} ${s.chapter}:${s.verse}–${e.chapter}:${verse(e.verse)}`;
}

/** Alias keys beyond the full names. Mirrors `ALIASES` in scripture.rs. */
const ALIASES: [string, number][] = [
  ["gen", 1], ["exod", 2], ["ex", 2], ["lev", 3], ["num", 4], ["deut", 5], ["dt", 5],
  ["josh", 6], ["judg", 7], ["jdg", 7], ["1sam", 9], ["1sm", 9], ["2sam", 10], ["2sm", 10],
  ["1kgs", 11], ["1ki", 11], ["2kgs", 12], ["2ki", 12], ["1chr", 13], ["1ch", 13],
  ["2chr", 14], ["2ch", 14], ["neh", 16], ["esth", 17], ["ps", 19], ["pss", 19],
  ["psalm", 19], ["pslm", 19], ["prov", 20], ["prv", 20], ["pr", 20], ["eccl", 21],
  ["ecc", 21], ["qoh", 21], ["song", 22], ["songofsongs", 22], ["songofsolomon", 22],
  ["sos", 22], ["cant", 22], ["canticles", 22], ["isa", 23], ["jer", 24], ["lam", 25],
  ["ezek", 26], ["eze", 26], ["dan", 27], ["hos", 28], ["obad", 31], ["ob", 31],
  ["jon", 32], ["mic", 33], ["nah", 34], ["hab", 35], ["zeph", 36], ["hag", 37],
  ["zech", 38], ["mal", 39], ["matt", 40], ["mt", 40], ["mk", 41], ["mrk", 41], ["lk", 42],
  ["luk", 42], ["jn", 43], ["jhn", 43], ["rom", 45], ["1cor", 46], ["2cor", 47], ["gal", 48],
  ["eph", 49], ["phil", 50], ["php", 50], ["col", 51], ["1thess", 52], ["1thes", 52],
  ["1th", 52], ["2thess", 53], ["2thes", 53], ["2th", 53], ["1tim", 54], ["1ti", 54],
  ["2tim", 55], ["2ti", 55], ["tit", 56], ["phlm", 57], ["phm", 57], ["heb", 58],
  ["jas", 59], ["jm", 59], ["1pet", 60], ["1pt", 60], ["2pet", 61], ["2pt", 61],
  ["1jn", 62], ["2jn", 63], ["3jn", 64], ["rev", 66], ["rv", 66],
];

/** Lowercase letters and digits only: "1 Cor." becomes "1cor". */
const squash = (text: string) => text.toLowerCase().replace(/[^a-z0-9]/g, "");

const NAMES: [string, number][] = [...BOOK_NAMES.map((n, i): [string, number] => [squash(n), i + 1]), ...ALIASES];

/** "I", "II", "First", "1st" and so on as a digit before the rest. */
function bookKey(words: string[]): string {
  const ordinals: Record<string, string> = { i: "1", "1st": "1", first: "1", ii: "2", "2nd": "2", second: "2", iii: "3", "3rd": "3", third: "3" };
  const digit = words.length > 1 ? ordinals[squash(words[0])] : undefined;
  return digit ? digit + squash(words.slice(1).join(" ")) : squash(words.join(" "));
}

/** A book number, or "unknown" or "ambiguous". */
function lookupBook(key: string): number | "unknown" | "ambiguous" {
  if (!key) return "unknown";
  const exact = NAMES.find(([name]) => name === key);
  if (exact) return exact[1];
  const matches = new Set(NAMES.filter(([name]) => name.startsWith(key)).map(([, n]) => n));
  if (matches.size === 0) return "unknown";
  return matches.size === 1 ? [...matches][0] : "ambiguous";
}

function count(text: string): number | null {
  if (!/^\d+$/.test(text)) return null;
  const n = Number(text);
  return n >= 1 && n < END_OF_CHAPTER ? n : null;
}

function point(text: string): [number, number | null] | null {
  const colon = text.indexOf(":");
  if (colon < 0) {
    const n = count(text);
    return n == null ? null : [n, null];
  }
  const a = count(text.slice(0, colon));
  const b = count(text.slice(colon + 1));
  return a == null || b == null ? null : [a, b];
}

/**
 * Reads a reference as the core's `parse_reference` does ("Jn 3:16-18",
 * "1 Cor 13", "Ps 23", "Jude 3"), or null when it is not one. The core
 * still has the last word: the library looks the passage up with it.
 */
export function parseReference(input: string): Passage | null {
  const text = input.trim();
  if (!text) return null;
  // The book is everything before the first digit after the first character.
  const digit = text.slice(1).search(/\d/);
  const split = digit < 0 ? text.length : digit + 1;
  const words = text.slice(0, split).split(/\s+/).filter(Boolean);

  let book: number | null = null;
  let used = 0;
  for (let n = words.length; n >= 1; n--) {
    const found = lookupBook(bookKey(words.slice(0, n)));
    if (found === "ambiguous") return null;
    if (found !== "unknown") {
      book = found;
      used = n;
      break;
    }
  }
  if (book == null || used < words.length) return null;

  const numbers = text
    .slice(split)
    .replace(/\s+/g, "")
    .replace(/[–—−]/g, "-")
    .replaceAll(".", ":");
  const cv = (chapter: number, verse: number) => ({ chapter, verse });
  const single = SINGLE_CHAPTER.has(book);
  if (!numbers) return single ? { book, start: cv(1, 1), end: cv(1, END_OF_CHAPTER) } : null;

  const dash = numbers.indexOf("-");
  const first = point(dash < 0 ? numbers : numbers.slice(0, dash));
  const second = dash < 0 ? null : point(numbers.slice(dash + 1));
  if (!first || (dash >= 0 && !second)) return null;

  let start, end;
  const [a, b] = first;
  if (b == null && single) {
    start = cv(1, a);
    end = !second ? cv(1, a) : second[1] == null ? cv(1, second[0]) : cv(second[0], second[1]);
  } else if (b == null) {
    if (second && second[1] != null) return null;
    start = cv(a, 1);
    end = cv(second ? second[0] : a, END_OF_CHAPTER);
  } else {
    start = cv(a, b);
    end = !second ? cv(a, b) : second[1] == null ? cv(a, second[0]) : cv(second[0], second[1]);
  }
  const before = end.chapter < start.chapter || (end.chapter === start.chapter && end.verse < start.verse);
  return before ? null : { book, start, end };
}
