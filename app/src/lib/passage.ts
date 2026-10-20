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
