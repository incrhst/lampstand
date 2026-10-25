// Keeps the phone's screen on while the remote is open. The Wake Lock API
// needs a secure context and the remote is served over plain HTTP
// (decision 0006, item 5), so this plays a tiny muted, looping, inline video,
// which phones treat as something being watched. It must start from a tap.

import video from "./wake.mp4";

let player: HTMLVideoElement | null = null;

/** Starts the video if it is not playing. Call from a tap. */
export function keepAwake() {
  if (!player) {
    player = document.createElement("video");
    player.src = video;
    player.muted = true;
    player.loop = true;
    player.playsInline = true;
    player.setAttribute("playsinline", "");
    player.setAttribute("aria-hidden", "true");
    // In the page but out of sight: some browsers will not play a video that
    // is not in the document.
    player.style.cssText = "position:fixed;width:1px;height:1px;opacity:0;pointer-events:none;bottom:0;left:0";
    document.body.appendChild(player);
    // A phone pauses the video when the page is hidden; start it again on return.
    document.addEventListener("visibilitychange", () => {
      if (document.visibilityState === "visible" && player?.paused) void player.play().catch(() => {});
    });
  }
  if (player.paused) void player.play().catch(() => {});
}
