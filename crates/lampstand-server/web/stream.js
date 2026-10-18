// OBS stream layout: live text as a lower third (or full), deck slides full
// frame, nothing at all in clear/black/logo. Crossfades between cues.
import { connect, cueKey, imageUrl, params, preloadImage } from "./shared/ws.js";

const FADE_MS = 200;
const layout = params.get("layout") === "full" ? "full" : "lowerthird";
const scale = Number.parseFloat(params.get("scale"));
if (Number.isFinite(scale) && scale > 0) document.documentElement.style.setProperty("--scale", String(scale));

const frame = document.getElementById("frame");
let current = null; // { key, el }
let generation = 0;

function frameWidth() {
  return Math.max(640, window.innerWidth * (window.devicePixelRatio || 1));
}

/** What the stream shows for a snapshot: the live cue in normal mode, else nothing. */
function visibleCue(s) {
  return s.mode === "normal" || s.mode == null ? s.live_cue ?? null : null;
}

function buildLayer(cue) {
  const layer = document.createElement("div");
  layer.className = "layer";
  if (cue.kind === "image") {
    const img = document.createElement("img");
    img.alt = "";
    img.src = imageUrl(cue.url, frameWidth());
    layer.append(img);
    return layer;
  }
  layer.classList.add(layout);
  const box = document.createElement("div");
  box.className = "box";
  const lines = document.createElement("div");
  lines.className = "lines";
  for (const line of cue.lines || []) {
    const p = document.createElement("p");
    p.textContent = line || " ";
    lines.append(p);
  }
  if (cue.footer) {
    const f = document.createElement("div");
    f.className = "footer";
    f.textContent = cue.footer;
    lines.append(f);
  }
  box.append(lines);
  layer.append(box);
  return layer;
}

/** Shrink text that would overflow its box (long scripture, big ?scale=). */
function shrinkToFit(layer) {
  const box = layer.querySelector(".box");
  const lines = layer.querySelector(".lines");
  if (!box || !lines) return;
  let size = parseFloat(getComputedStyle(lines).fontSize);
  for (let i = 0; i < 40 && box.scrollHeight > box.clientHeight + 1 && size > 10; i++) {
    size *= 0.94;
    lines.style.fontSize = `${size}px`;
  }
}

async function show(cue) {
  const key = cueKey(cue);
  if (current?.key === key) return;
  const gen = ++generation;

  let layer = null;
  if (cue) {
    if (cue.kind === "image") {
      // Wait (briefly) for the decoded image so the fade never shows a half-loaded slide.
      await Promise.race([
        preloadImage(imageUrl(cue.url, frameWidth())),
        new Promise((r) => setTimeout(r, 1500)),
      ]);
      if (gen !== generation) return;
    }
    layer = buildLayer(cue);
    frame.append(layer);
    shrinkToFit(layer);
    // Commit the starting opacity before fading in.
    void layer.offsetWidth;
    layer.classList.add("shown");
  }

  const old = current?.el;
  if (old) {
    old.classList.remove("shown");
    setTimeout(() => old.remove(), FADE_MS + 50);
  }
  current = { key, el: layer };
}

connect({
  role: "stream",
  name: params.get("name") || "OBS",
  onState(s) {
    show(visibleCue(s));
    const next = s.next_cue;
    if (next?.kind === "image") preloadImage(imageUrl(next.url, frameWidth()));
  },
  // No reconnecting indicator on the stream: whatever is shown stays until the
  // server is back.
});
