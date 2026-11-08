// What Present says about the training videos (#190). They are not in the
// installer: they download the first time a training service is opened,
// and the rest of each training service runs without them.

import type { TrainingVideos } from "./api";

/** The line to show, or null when every training video is here. */
export function trainingVideosLine(videos: TrainingVideos | null): string | null {
  if (!videos) return null;
  const state = videos.state;
  if (state === "Ready") return null;
  if (typeof state === "object" && "Downloading" in state) {
    const { done, total } = state.Downloading;
    if (total > 0) {
      const percent = Math.min(100, Math.floor((done / total) * 100));
      return `Downloading the training videos: ${percent}%`;
    }
    return "Downloading the training videos…";
  }
  return videos.message || null;
}

/** The download is going, so the line is progress, not a problem. */
export function trainingVideosBusy(videos: TrainingVideos | null): boolean {
  const state = videos?.state;
  return typeof state === "object" && state !== null && "Downloading" in state;
}
