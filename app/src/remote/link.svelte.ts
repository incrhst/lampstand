// The remote's one way to the laptop: the remote socket (`WS /remote/ws`),
// in place of the desktop's Tauri IPC. Requests are the same `Request`s
// Present sends; the laptop's gate refuses anything but the slides and the
// sound. What the laptop says is held here, and wiped the moment the link
// drops, so the page never shows something old as if it were on screen now.

import type { Event, LoadedService, Request, Response, SoundView } from "../lib/api";
import { backoff, type LiveState } from "../lib/remote.ts";

export type LinkState = "connecting" | "online" | "offline" | "unpaired" | "revoked";

/** Close code the laptop sends when this phone is revoked. */
const REVOKED = 4401;
/** The laptop says something every 5 s; this much silence means the link is gone. */
const SILENCE_MS = 12_000;
const ANSWER_MS = 8_000;

type Pending = { resolve: (r: Response) => void; reject: (e: Error) => void; timer: ReturnType<typeof setTimeout> };

class Link {
  state: LinkState = $state("connecting");
  /** The phone's name, as paired. */
  name = $state("");
  loaded: LoadedService | null = $state(null);
  live: LiveState | null = $state(null);
  sound: SoundView | null = $state(null);
  /** When `sound` was said, for moving the position on between messages. */
  soundAt = $state(0);
  /** Failed tries since the link was last up, for the backoff. */
  attempts = $state(0);

  private ws: WebSocket | null = null;
  private heard = 0;
  private nextId = 1;
  private pending = new Map<number, Pending>();
  private retry: ReturnType<typeof setTimeout> | null = null;
  private watchdog: ReturnType<typeof setInterval> | null = null;

  /** Opens the socket, and keeps it open until revoked. Again after pairing anew. */
  start() {
    if (this.watchdog) {
      if (this.ws) return;
      if (this.retry) clearTimeout(this.retry);
      this.attempts = 0;
      this.state = "connecting";
      this.connect();
      return;
    }
    this.watchdog = setInterval(() => {
      if (this.state === "online" && Date.now() - this.heard > SILENCE_MS) this.drop();
    }, 2000);
    // A phone pauses a page it is not showing; on its return, try at once.
    document.addEventListener("visibilitychange", () => {
      if (document.visibilityState === "visible") this.now();
    });
    window.addEventListener("online", () => this.now());
    this.connect();
  }

  /** Reconnects without waiting out the backoff, if the link is down. */
  now() {
    if (this.state !== "offline" && this.state !== "connecting") return;
    if (this.ws) return;
    if (this.retry) clearTimeout(this.retry);
    this.retry = null;
    this.connect();
  }

  private connect() {
    this.retry = null;
    if (this.state !== "offline") this.state = "connecting";
    const scheme = location.protocol === "https:" ? "wss" : "ws";
    const ws = new WebSocket(`${scheme}://${location.host}/remote/ws`);
    this.ws = ws;
    let opened = false;
    ws.onopen = () => {
      opened = true;
      this.heard = Date.now();
    };
    ws.onmessage = (m) => {
      if (this.ws !== ws) return;
      this.heard = Date.now();
      this.receive(String(m.data));
    };
    ws.onclose = (e) => {
      if (this.ws !== ws) return;
      this.ws = null;
      this.forget();
      if (e.code === REVOKED) {
        this.state = "revoked";
        return;
      }
      // The socket refused before opening: not paired any more, or the
      // laptop is not there. The session check tells which.
      if (!opened) {
        void this.check();
        return;
      }
      this.later();
    };
  }

  private async check() {
    try {
      const r = await fetch("/remote/session", { cache: "no-store" });
      if (r.status === 401) {
        this.state = "unpaired";
        return;
      }
    } catch {
      // No answer at all: the laptop or the Wi-Fi is away.
    }
    this.later();
  }

  private later() {
    this.state = "offline";
    const wait = backoff(this.attempts);
    this.attempts++;
    this.retry = setTimeout(() => this.connect(), wait);
  }

  /** The link is gone though the socket has not said so (a phone's Wi-Fi drops silently). */
  private drop() {
    const ws = this.ws;
    this.ws = null;
    this.forget();
    ws?.close();
    this.later();
  }

  /** Nothing the laptop said is true any more. */
  private forget() {
    this.loaded = null;
    this.live = null;
    this.sound = null;
    for (const p of this.pending.values()) {
      clearTimeout(p.timer);
      p.reject(new Error("Not connected to the laptop."));
    }
    this.pending.clear();
  }

  private receive(text: string) {
    let message: { hello?: { name: string }; event?: Event; id?: number; response?: Response; beat?: number };
    try {
      message = JSON.parse(text);
    } catch {
      return;
    }
    if (message.hello) {
      this.name = message.hello.name;
      this.state = "online";
      this.attempts = 0;
    } else if (message.event) {
      this.apply(message.event);
    } else if (message.id != null && message.response) {
      const p = this.pending.get(message.id);
      if (!p) return;
      this.pending.delete(message.id);
      clearTimeout(p.timer);
      p.resolve(message.response);
    }
  }

  private apply(event: Event) {
    switch (event.type) {
      case "Live":
        this.live = event.data;
        break;
      case "Loaded":
        this.loaded = event.data;
        break;
      case "Unloaded":
        this.loaded = null;
        break;
      case "Sound":
        this.sound = event.data;
        this.soundAt = Date.now();
        break;
    }
  }

  /** Sends a request and returns the laptop's answer; throws its error in words. */
  async send(request: Request): Promise<Response> {
    const ws = this.ws;
    if (!ws || this.state !== "online") throw new Error("Not connected to the laptop.");
    const id = this.nextId++;
    const answer = new Promise<Response>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error("The laptop did not answer. Try again."));
      }, ANSWER_MS);
      this.pending.set(id, { resolve, reject, timer });
    });
    ws.send(JSON.stringify({ id, request }));
    const response = await answer;
    if (response.type === "Error") throw new Error(response.data.message);
    if (response.type === "Sound") {
      this.sound = response.data;
      this.soundAt = Date.now();
    }
    return response;
  }
}

export const link = new Link();
