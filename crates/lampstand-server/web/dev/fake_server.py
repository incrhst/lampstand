#!/usr/bin/env python3
"""Fake Lampstand server for working on the stage and stream pages.

Python standard library only. Serves crates/lampstand-server/web/, a WebSocket
at /ws that speaks the v0.1 protocol (crates/lampstand-show/src/protocol.rs),
generated PNGs for /media/deck/... and /media/logo.png, and a few dev controls:

    GET /dev/state/<n>     show scripted snapshot n (0-based) to every client
    GET /dev/next          next scripted snapshot
    GET /dev/drop?secs=5   close every WebSocket and refuse new ones for a while
    GET /dev/auto?secs=4   step through the script every N seconds (secs=0 stops)

Run:  python3 crates/lampstand-server/web/dev/fake_server.py [--port 7420] [--skew SECS]
Then: http://127.0.0.1:7420/stage  and  http://127.0.0.1:7420/stream
"""

import argparse
import base64
import hashlib
import json
import socket
import struct
import threading
import time
import zlib
from http import HTTPStatus
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlparse

WEB = Path(__file__).resolve().parent.parent
WS_GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"

SONG = {"id": "s1", "kind": "song", "title": "Amazing Grace", "cues": ["V1", "V2", "C"], "problem": None}
JOHN = {"id": "b1", "kind": "scripture", "title": "John 3:16-17", "cues": ["Jn 3:16", "Jn 3:17"], "problem": None}
DECK = {"id": "d1", "kind": "deck", "title": "Sermon: Light of the World", "cues": ["Slide 1", "Slide 2 · 1/2", "Slide 2 · 2/2"], "problem": None}
ITEMS = [SONG, JOHN, DECK]
FOOTER = "Amazing Grace · John Newton · Public domain"


def text(lines, label, footer=FOOTER, notes=None):
    return {"kind": "text", "lines": lines, "footer": footer, "label": label, "notes": notes}


def image(slide, step, label, notes=None):
    return {"kind": "image", "url": f"/media/deck/3f2a/{slide}/{step}.png", "aspect": 16 / 9, "label": label, "notes": notes}


V1 = text(["Amazing grace, how sweet the sound", "That saved a wretch like me", "I once was lost, but now am found", "Was blind, but now I see"], "V1")
V2 = text(["'Twas grace that taught my heart to fear", "And grace my fears relieved", "How precious did that grace appear", "The hour I first believed"], "V2")
JN16 = text(["For God so loved the world, that he gave his only begotten Son,",
             "that whosoever believeth in him should not perish, but have everlasting life."], "Jn 3:16", footer="John 3:16 KJV")
NOTES1 = ("Welcome everyone, especially visitors.\n\nPoint one: light reveals.\n"
          "Story about the lighthouse keeper on the north coast.\n\nPause here for the video.\n\n"
          "Point two: light guides. Read Psalm 119:105 from memory.\n\nPoint three: light warms.\n"
          "Invite people to stay for coffee afterwards.")
SL1 = image(0, 0, "Slide 1", notes=NOTES1)
SL2 = image(1, 0, "Slide 2 · 1/2", notes="Point two: light guides. Read Psalm 119:105.")

# (description, live pos, mode, live cue, next cue, item index for timers)
SCRIPT = [
    ("nothing live", None, "normal", None, V1),
    ("song text", (0, 0), "normal", V1, V2),
    ("song text v2", (0, 1), "normal", V2, JN16),
    ("scripture text", (1, 0), "normal", JN16, SL1),
    ("deck image with notes", (2, 0), "normal", SL1, SL2),
    ("deck image, black", (2, 0), "black", SL1, SL2),
    ("deck image, clear", (2, 0), "clear", SL1, SL2),
    ("deck step 2, logo", (2, 1), "logo", SL2, None),
]

lock = threading.Lock()
SKEW = {"secs": 0.0}


def server_time():
    """This server's clock, optionally skewed to test clients that sync to it."""
    return time.time() + SKEW["secs"]


clients = set()
state = {"index": 1, "revision": 0, "refuse_until": 0.0, "auto": 0.0,
         "service_started_at": None, "item_started_at": None, "item": None}


def snapshot():
    _, live, mode, live_cue, next_cue = SCRIPT[state["index"]]
    now = int(server_time() * 1000)
    if live is not None:
        if state["service_started_at"] is None:
            state["service_started_at"] = now - 23 * 60 * 1000  # pretend we are 23 min in
        if state["item"] != live[0]:
            state["item"] = live[0]
            state["item_started_at"] = now - 75 * 1000
    return {
        "type": "state",
        "service_name": "Sunday 10am",
        "items": ITEMS,
        "preview": None if live is None else {"item": live[0], "cue": live[1] + 1},
        "live": None if live is None else {"item": live[0], "cue": live[1]},
        "mode": mode,
        "preview_cue": next_cue,
        "live_cue": live_cue,
        "next_cue": next_cue,
        "logo": "/media/logo.png",
        "service_started_at": state["service_started_at"] if live else None,
        "item_started_at": state["item_started_at"] if live else None,
        "revision": state["revision"],
    }


def ws_frame(payload: bytes, opcode=0x1) -> bytes:
    n = len(payload)
    if n < 126:
        head = struct.pack("!BB", 0x80 | opcode, n)
    elif n < 1 << 16:
        head = struct.pack("!BBH", 0x80 | opcode, 126, n)
    else:
        head = struct.pack("!BBQ", 0x80 | opcode, 127, n)
    return head + payload


def ws_read(sock):
    """One client frame -> (opcode, payload), or None when the socket closes."""
    def exact(k):
        buf = b""
        while len(buf) < k:
            chunk = sock.recv(k - len(buf))
            if not chunk:
                return None
            buf += chunk
        return buf
    head = exact(2)
    if head is None:
        return None
    opcode, n = head[0] & 0x0F, head[1] & 0x7F
    if n in (126, 127):
        ext = exact(2 if n == 126 else 8)
        if ext is None:
            return None
        n = struct.unpack("!H" if n == 126 else "!Q", ext)[0]
    mask = exact(4) if head[1] & 0x80 else b"\0\0\0\0"
    data = exact(n) if n else b""
    if data is None or mask is None:
        return None
    return opcode, bytes(b ^ mask[i % 4] for i, b in enumerate(data))


def broadcast():
    with lock:
        state["revision"] += 1
        msg = ws_frame(json.dumps(snapshot()).encode())
        for c in list(clients):
            try:
                c.sendall(msg)
            except OSError:
                clients.discard(c)
    print(f"[fake] -> state {state['index']}: {SCRIPT[state['index']][0]} (rev {state['revision']})", flush=True)


def drop_all():
    with lock:
        for c in list(clients):
            try:
                c.sendall(ws_frame(b"\x03\xe9", opcode=0x8))
                c.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
        clients.clear()


def auto_loop():
    while True:
        time.sleep(0.25)
        secs = state["auto"]
        if secs and time.time() >= state.get("auto_next", 0):
            state["auto_next"] = time.time() + secs
            state["index"] = (state["index"] + 1) % len(SCRIPT)
            broadcast()


def png(width, height, slide, step):
    """A simple slide: coloured background, a title bar and a bullet block per step."""
    palettes = [((24, 48, 96), (250, 200, 80)), ((40, 90, 60), (240, 240, 240)), ((90, 30, 60), (255, 220, 160))]
    bg, fg = palettes[slide % len(palettes)]
    rows = []
    for y in range(height):
        fy = y / height
        shade = tuple(int(c * (0.7 + 0.5 * fy)) for c in bg)
        row = bytearray(bytes(shade) * width)
        bands = [(0.12, 0.24, 0.08, 0.80)] + [(0.38 + i * 0.16, 0.46 + i * 0.16, 0.12, 0.70) for i in range(step + 2)]
        for top, bottom, left, right in bands:
            if top <= fy < bottom:
                x0, x1 = int(left * width), int(right * width)
                row[x0 * 3:x1 * 3] = bytes(fg) * (x1 - x0)
        rows.append(b"\0" + bytes(row))
    raw = zlib.compress(b"".join(rows), 6)

    def chunk(kind, data):
        return struct.pack("!I", len(data)) + kind + data + struct.pack("!I", zlib.crc32(kind + data) & 0xFFFFFFFF)
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack("!IIBBBBB", width, height, 8, 2, 0, 0, 0))
            + chunk(b"IDAT", raw) + chunk(b"IEND", b""))


class Handler(SimpleHTTPRequestHandler):
    def __init__(self, *a, **kw):
        super().__init__(*a, directory=str(WEB), **kw)

    def date_time_string(self, timestamp=None):
        return super().date_time_string(server_time() if timestamp is None else timestamp)

    def log_message(self, format, *args):
        if "/media/" in self.path or "/dev/" in self.path:
            super().log_message(format, *args)

    def end_headers(self):
        self.send_header("Cache-Control", "no-store")
        super().end_headers()

    def send_bytes(self, body, ctype):
        self.send_response(HTTPStatus.OK)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        url = urlparse(self.path)
        q = parse_qs(url.query)
        path = url.path
        if path == "/ws":
            return self.websocket()
        if path in ("/stage", "/stream"):
            self.path = path + ".html" + (f"?{url.query}" if url.query else "")
            return super().do_GET()
        if path.startswith("/media/deck/"):
            parts = path.split("/")  # ['', 'media', 'deck', deck, slide, 'step.png']
            slide, step = int(parts[4]), int(parts[5].split(".")[0])
            w = min(3840, max(64, int(q.get("w", ["1920"])[0])))
            time.sleep(0.05)
            return self.send_bytes(png(w, round(w * 9 / 16), slide, step), "image/png")
        if path == "/media/logo.png":
            return self.send_bytes(png(480, 270, 2, 0), "image/png")
        if path in ("/dev/next", "/dev/drop", "/dev/auto") or path.startswith("/dev/state/"):
            return self.dev(path, q)
        return super().do_GET()

    def dev(self, path, q):
        secs = float(q.get("secs", ["5"])[0])
        if path == "/dev/next":
            state["index"] = (state["index"] + 1) % len(SCRIPT)
            broadcast()
        elif path.startswith("/dev/state/"):
            state["index"] = int(path.rsplit("/", 1)[1]) % len(SCRIPT)
            broadcast()
        elif path == "/dev/drop":
            state["refuse_until"] = time.time() + secs
            drop_all()
        elif path == "/dev/auto":
            state["auto"] = secs if secs > 0 else 0.0
        else:
            self.send_error(404)
            return
        self.send_bytes(json.dumps({"index": state["index"], "state": SCRIPT[state["index"]][0]}).encode(), "application/json")

    def websocket(self):
        key = self.headers.get("Sec-WebSocket-Key")
        if not key or time.time() < state["refuse_until"]:
            self.send_error(503 if key else 400)
            return
        accept = base64.b64encode(hashlib.sha1((key + WS_GUID).encode()).digest()).decode()
        self.send_response(HTTPStatus.SWITCHING_PROTOCOLS)
        self.send_header("Upgrade", "websocket")
        self.send_header("Connection", "Upgrade")
        self.send_header("Sec-WebSocket-Accept", accept)
        super(SimpleHTTPRequestHandler, self).end_headers()
        sock = self.connection
        hello = ws_read(sock)
        if hello and hello[0] == 0x1:
            print(f"[fake] hello {hello[1].decode()}", flush=True)
        with lock:
            clients.add(sock)
            state["revision"] += 1
            sock.sendall(ws_frame(json.dumps(snapshot()).encode()))
        try:
            while True:
                frame = ws_read(sock)
                if frame is None or frame[0] == 0x8:
                    break
                if frame[0] == 0x9:
                    sock.sendall(ws_frame(frame[1], opcode=0xA))
        except OSError:
            pass
        finally:
            with lock:
                clients.discard(sock)
            self.close_connection = True


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=7420)
    ap.add_argument("--skew", type=float, default=0, help="run the server clock this many seconds off")
    ap.add_argument("--auto", type=float, default=0, help="step through the script every N seconds")
    args = ap.parse_args()
    state["auto"] = args.auto
    SKEW["secs"] = args.skew
    threading.Thread(target=auto_loop, daemon=True).start()
    server = ThreadingHTTPServer(("127.0.0.1", args.port), Handler)
    server.daemon_threads = True
    print(f"[fake] http://127.0.0.1:{args.port}/stage  /stream  (script: {len(SCRIPT)} states)", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
