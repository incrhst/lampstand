// Stage display: live cue large, next cue, speaker notes, clock and timers.
import { connect, cueKey, formatElapsed, imageUrl, params, preloadImage, serverNow, syncServerClock } from "./shared/ws.js";

const $ = (id) => document.getElementById(id);
const el = {
  itemTitle: $("item-title"),
  liveLabel: $("live-label"),
  nextLabel: $("next-label"),
  itemElapsed: $("item-elapsed"),
  serviceElapsed: $("service-elapsed"),
  clock: $("clock"),
  live: $("live-cue"),
  livePanel: document.querySelector(".panel.live"),
  next: $("next-cue"),
  notes: $("notes"),
  banner: $("mode-banner"),
  reconnecting: $("reconnecting"),
};

const BANNERS = { black: "BLACK", clear: "CLEAR", logo: "LOGO" };
const use24h = params.get("clock") === "24";

let snapshot = null;
const shown = { live: null, next: null };

function itemTitle(s) {
  if (!s.live) return s.service_name || "Nothing live";
  return s.items?.[s.live.item]?.title ?? "";
}

/** Draw a cue into a panel; skipped when it has not changed. */
function drawCue(slot, container, cue, emptyText) {
  const key = cueKey(cue) || `empty|${emptyText}`;
  if (shown[slot] === key) return;
  shown[slot] = key;
  container.replaceChildren();

  if (!cue) {
    const p = document.createElement("div");
    p.className = "empty";
    p.textContent = emptyText;
    container.append(p);
    return;
  }

  if (cue.kind === "image") {
    const img = document.createElement("img");
    img.alt = cue.label || "Slide";
    img.src = imageUrl(cue.url, widthFor(container));
    container.append(img);
    return;
  }

  const block = document.createElement("div");
  block.className = "lines";
  for (const line of cue.lines || []) {
    const p = document.createElement("p");
    p.textContent = line || " ";
    block.append(p);
  }
  if (cue.footer) {
    const f = document.createElement("div");
    f.className = "footer";
    f.textContent = cue.footer;
    block.append(f);
  }
  container.append(block);
  fit(container, block);
}

function widthFor(container) {
  return Math.max(320, container.clientWidth * (window.devicePixelRatio || 1));
}

/**
 * Fit `block` inside `container`. Lyrics read best one authored line per row, so
 * keep lines unwrapped unless that makes the text much smaller than wrapping would.
 */
function fit(container, block) {
  const style = getComputedStyle(container);
  const availH = container.clientHeight - parseFloat(style.paddingTop) - parseFloat(style.paddingBottom);
  if (availH <= 0 || container.clientWidth <= 0) return;
  block.style.whiteSpace = "nowrap";
  const unwrapped = largestFitting(block, availH);
  block.style.whiteSpace = "";
  const wrapped = largestFitting(block, availH);
  const useUnwrapped = unwrapped >= wrapped * 0.6;
  block.style.whiteSpace = useUnwrapped ? "nowrap" : "";
  block.style.fontSize = `${Math.floor(useUnwrapped ? unwrapped : wrapped)}px`;
}

function largestFitting(block, availH) {
  let lo = 8;
  let hi = Math.max(lo, Math.min(availH, 200));
  while (hi - lo > 0.5) {
    const mid = (lo + hi) / 2;
    block.style.fontSize = `${mid}px`;
    if (block.scrollHeight <= availH && block.scrollWidth <= block.clientWidth) lo = mid;
    else hi = mid;
  }
  return lo;
}

function render() {
  const s = snapshot;
  if (!s) return;
  const live = s.live_cue ?? null;
  const next = s.next_cue ?? null;

  el.itemTitle.textContent = itemTitle(s);
  el.liveLabel.textContent = live?.label ?? "";
  el.nextLabel.textContent = next?.label ? `· ${next.label}` : "";

  drawCue("live", el.live, live, "Nothing live");
  drawCue("next", el.next, next, "End of service");

  const banner = BANNERS[s.mode];
  el.banner.hidden = !banner;
  el.banner.textContent = banner ?? "";
  el.banner.dataset.mode = s.mode;
  el.livePanel.classList.toggle("dimmed", Boolean(banner));

  const notes = live?.notes?.trim();
  if (el.notes.dataset.text !== (notes ?? "")) {
    el.notes.dataset.text = notes ?? "";
    el.notes.textContent = notes || "No notes";
    el.notes.classList.toggle("none", !notes);
    el.notes.scrollTop = 0;
  }

  if (next?.kind === "image") preloadImage(imageUrl(next.url, widthFor(el.live)));
  tick();
}

function tick() {
  // Server time, so the clock and timers agree with the operator even when
  // this device's clock is wrong.
  const now = serverNow();
  el.clock.textContent = new Date(now).toLocaleTimeString([], {
    hour: "numeric",
    minute: "2-digit",
    hour12: !use24h,
  });
  const s = snapshot;
  el.itemElapsed.textContent = s?.item_started_at ? formatElapsed(now - s.item_started_at) : "–:––";
  el.serviceElapsed.textContent = s?.service_started_at ? formatElapsed(now - s.service_started_at) : "–:––";
}

// Text sizes and image widths depend on the panel size.
let resizeTimer = null;
window.addEventListener("resize", () => {
  clearTimeout(resizeTimer);
  resizeTimer = setTimeout(() => {
    shown.live = shown.next = null;
    render();
  }, 100);
});

setInterval(tick, 250);
tick();
const resyncClock = syncServerClock();

connect({
  role: "stage",
  name: params.get("name") || "Stage",
  onState(s) {
    snapshot = s;
    render();
  },
  onStatus(connected) {
    isConnected = connected;
    el.reconnecting.hidden = connected;
    // The server may have restarted on another machine or with a new clock.
    if (connected && wasConnected) resyncClock();
    wasConnected ||= connected;
  },
});
// Give the first connect a moment before saying we are reconnecting.
let isConnected = false;
let wasConnected = false;
setTimeout(() => {
  if (!isConnected) el.reconnecting.hidden = false;
}, 1500);
