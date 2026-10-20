// What the planner shows for decks and media files: which files the
// pickers offer, whether a file is a video, and where a deck came from.

import type { DeckSource } from "./api";

/** PowerPoint, Keynote and Google Slides all export this. */
export const deckExtensions = ["pptx"];
/** The extensions the core plays as video (`handler.rs`, `load_item`). */
export const videoExtensions = ["mp4", "mov", "m4v", "webm", "mkv"];
export const imageExtensions = ["png", "jpg", "jpeg", "gif", "webp", "bmp"];

function extension(name: string): string {
  const dot = name.lastIndexOf(".");
  return dot < 0 ? "" : name.slice(dot + 1).toLowerCase();
}

/** A media item plays as video when its name says so, else as an image. */
export function isVideo(name: string): boolean {
  return videoExtensions.includes(extension(name));
}

/** "Video" or "Image", for a media item's subtitle. */
export function mediaKind(name: string): "Video" | "Image" {
  return isVideo(name) ? "Video" : "Image";
}

/** Where a deck came from, for its subtitle. */
export function deckSourceLabel(source: DeckSource): string {
  if ("File" in source) return "From a file";
  if ("GoogleSlidesLink" in source) return "Google Slides link";
  if ("GoogleDrive" in source) return "Google Drive";
  if ("OneDriveLink" in source) return "OneDrive link";
  return `Uploaded by ${source.GuestUpload.uploader}`;
}

/** The single path a dialog returned, or null if it was cancelled. */
export function onePath(picked: string | string[] | null): string | null {
  if (picked == null) return null;
  return Array.isArray(picked) ? (picked[0] ?? null) : picked;
}
