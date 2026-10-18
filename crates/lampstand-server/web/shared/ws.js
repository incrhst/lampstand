// Lampstand WebSocket client shared by the read-only pages (stage, stream).
//
// Protocol: crates/lampstand-show/src/protocol.rs (source of truth) and
// docs/v0.1.md. The server sends `{"type":"state", ...Snapshot}` on connect and
// after every change; clients say `{"type":"hello","role":...,"name":...}` first.
// No dependencies, no external requests.

const RETRY_MS = 1000;

/** The `/ws` URL on the server that served this page. */
export function wsUrl() {
  const scheme = location.protocol === "https:" ? "wss:" : "ws:";
  return `${scheme}//${location.host}/ws`;
}

/**
 * Connect, say hello, and keep reconnecting every second until `close()`.
 *
 * - `onState(snapshot)` gets each snapshot whose `revision` is newer than the
 *   last one seen on this connection (stale and duplicate frames are dropped).
 * - `onStatus(connected)` fires when the connection opens or drops.
 */
export function connect({ role, name = role, onState, onStatus = () => {} }) {
  let ws = null;
  let timer = null;
  let closed = false;
  let lastRevision = -1;
  let connected = false;

  const setConnected = (value) => {
    if (connected === value) return;
    connected = value;
    onStatus(value);
  };

  const schedule = () => {
    if (closed || timer) return;
    timer = setTimeout(() => {
      timer = null;
      open();
    }, RETRY_MS);
  };

  const open = () => {
    if (closed) return;
    try {
      ws = new WebSocket(wsUrl());
    } catch {
      schedule();
      return;
    }
    ws.addEventListener("open", () => {
      // A restarted server starts counting revisions again.
      lastRevision = -1;
      ws.send(JSON.stringify({ type: "hello", role, name }));
      setConnected(true);
    });
    ws.addEventListener("message", (event) => {
      const snapshot = parseState(event.data);
      if (!snapshot) return;
      const revision = Number(snapshot.revision ?? 0);
      if (revision <= lastRevision) return;
      lastRevision = revision;
      onState(snapshot);
    });
    ws.addEventListener("close", () => {
      ws = null;
      setConnected(false);
      schedule();
    });
    // `close` always follows `error`; reconnect happens there.
    ws.addEventListener("error", () => {});
  };

  // Browsers throttle timers in background tabs; retry as soon as we are visible.
  document.addEventListener("visibilitychange", () => {
    if (!document.hidden && !ws && !closed) {
      clearTimeout(timer);
      timer = null;
      open();
    }
  });

  open();
  return {
    close() {
      closed = true;
      clearTimeout(timer);
      ws?.close();
    },
  };
}

/**
 * Pull the Snapshot out of a server frame. `ServerMessage::State` is an
 * internally tagged newtype, so the Snapshot fields sit next to `type`; a
 * nested `snapshot`/`state` object is accepted too. Returns null for anything else.
 */
export function parseState(data) {
  let msg;
  try {
    msg = JSON.parse(data);
  } catch {
    return null;
  }
  if (!msg || msg.type !== "state") {
    if (msg?.type === "error") console.warn("lampstand:", msg.message);
    return null;
  }
  if (msg.snapshot && typeof msg.snapshot === "object") return msg.snapshot;
  if (msg.state && typeof msg.state === "object") return msg.state;
  return msg;
}

/** A deck image URL at `width` device pixels (the server renders and caches it). */
export function imageUrl(url, width) {
  const w = Math.max(64, Math.round(width));
  return `${url}${url.includes("?") ? "&" : "?"}w=${w}`;
}

const preloaded = new Map();

/**
 * Fetch and decode an image ahead of time. Resolves to the loaded
 * `HTMLImageElement` (or null on error). Keeps the last few around.
 */
export function preloadImage(src) {
  if (preloaded.has(src)) return preloaded.get(src);
  const img = new Image();
  img.decoding = "async";
  img.src = src;
  const done = img.decode().then(
    () => img,
    () => null,
  );
  preloaded.set(src, done);
  while (preloaded.size > 8) preloaded.delete(preloaded.keys().next().value);
  return done;
}

/** Stable key for a cue, so pages only redraw when what they show changes. */
export function cueKey(cue) {
  if (!cue) return "";
  if (cue.kind === "image") return `i|${cue.url}`;
  return `t|${(cue.lines || []).join("\n")}|${cue.footer ?? ""}`;
}

/** "1:05", "12:34", "1:02:03" from milliseconds. */
export function formatElapsed(ms) {
  if (ms == null || !Number.isFinite(ms) || ms < 0) return "–:––";
  const total = Math.floor(ms / 1000);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = String(total % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${s}` : `${m}:${s}`;
}

/** Query parameters of this page as an object-ish accessor. */
export const params = new URLSearchParams(location.search);
