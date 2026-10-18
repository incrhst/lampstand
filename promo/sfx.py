"""Synthesise the promo's sound effects and ambient bed. Standard library only.

Every cue is placed from src/timeline.json, the same file the video reads,
so sounds land on the frame where their animation starts.

    python3 sfx.py            # writes public/sfx.wav
"""

import json
import math
import random
import struct
import wave
from pathlib import Path

ROOT = Path(__file__).parent
SR = 44100
random.seed(7)

T = json.loads((ROOT / "src" / "timeline.json").read_text())
FPS = T["fps"]
SC = T["scenes"]
EV = T["events"]
TOTAL = (SC["end"]["from"] + SC["end"]["dur"]) / FPS
N = int(TOTAL * SR) + SR

mix = [0.0] * N


def at(scene, frame):
    """Absolute time in seconds of a frame inside a scene."""
    return (SC[scene]["from"] + frame) / FPS


def add(t, buf, gain=1.0):
    start = int(t * SR)
    for i, v in enumerate(buf):
        j = start + i
        if 0 <= j < N:
            mix[j] += v * gain


def env_exp(n, decay):
    return [math.exp(-i / (decay * SR)) for i in range(n)]


# ---------- voices ----------

def tick(freq=2400.0, length=0.035):
    n = int(length * SR)
    out = []
    for i in range(n):
        t = i / SR
        e = math.exp(-t / 0.006)
        out.append(e * (0.6 * math.sin(2 * math.pi * freq * t) + 0.25 * (random.random() * 2 - 1)))
    return out


def pop(pitch=520.0, length=0.16):
    n = int(length * SR)
    out, ph = [], 0.0
    for i in range(n):
        t = i / SR
        f = pitch * (1 + 0.9 * math.exp(-t / 0.018))
        ph += 2 * math.pi * f / SR
        e = (1 - math.exp(-t / 0.002)) * math.exp(-t / 0.045)
        out.append(e * math.sin(ph))
    return out


def click(length=0.06):
    """Clicker press: a short bright transient over a soft low knock."""
    n = int(length * SR)
    out, prev = [], 0.0
    for i in range(n):
        t = i / SR
        w = random.random() * 2 - 1
        hp = w - prev  # crude high-pass
        prev = w
        out.append(math.exp(-t / 0.004) * 0.5 * hp + math.exp(-t / 0.02) * 0.6 * math.sin(2 * math.pi * 180 * t))
    return out


def whoosh(length=0.6, f0=300, f1=3000, rise=0.6):
    """Noise through a swept one-pole low-pass, with a bell-shaped swell."""
    n = int(length * SR)
    out, lp, lp2 = [], 0.0, 0.0
    for i in range(n):
        x = i / n
        f = f0 * (f1 / f0) ** x
        a = 1 - math.exp(-2 * math.pi * f / SR)
        w = random.random() * 2 - 1
        lp += a * (w - lp)
        lp2 += a * (lp - lp2)
        e = (x / rise) ** 2 if x < rise else ((1 - x) / (1 - rise)) ** 1.5
        out.append(lp2 * e * 2.2)
    return out


def chime(base=880.0, length=1.8):
    partials = [(1.0, 1.0, 1.2), (2.0, 0.5, 0.7), (2.76, 0.35, 0.5), (5.4, 0.15, 0.25), (3.0, 0.2, 0.9)]
    n = int(length * SR)
    out = []
    for i in range(n):
        t = i / SR
        att = min(1.0, t / 0.004)
        v = sum(a * math.exp(-t / d) * math.sin(2 * math.pi * base * r * t) for r, a, d in partials)
        out.append(att * v * 0.45)
    return out


def blip(f0=700, f1=1400, length=0.12):
    n = int(length * SR)
    out, ph = [], 0.0
    for i in range(n):
        x = i / n
        ph += 2 * math.pi * (f0 + (f1 - f0) * x) / SR
        out.append(math.sin(ph) * math.sin(math.pi * x) ** 0.6)
    return out


def thud(length=0.5):
    """Low 'something is wrong' knock with a short detuned buzz."""
    n = int(length * SR)
    out = []
    for i in range(n):
        t = i / SR
        body = math.exp(-t / 0.12) * math.sin(2 * math.pi * (60 + 30 * math.exp(-t / 0.03)) * t)
        buzz = math.exp(-t / 0.08) * 0.25 * (math.copysign(1, math.sin(2 * math.pi * 110 * t)) + math.copysign(1, math.sin(2 * math.pi * 116.5 * t)))
        out.append(body + buzz * 0.5)
    return out


def boom(length=2.2):
    n = int(length * SR)
    return [math.exp(-(i / SR) / 0.5) * math.sin(2 * math.pi * (46 + 20 * math.exp(-(i / SR) / 0.08)) * (i / SR)) for i in range(n)]


def riser(length=1.0):
    n = int(length * SR)
    w = whoosh(length, 200, 6000, rise=0.92)
    out, ph = [], 0.0
    for i in range(n):
        x = i / n
        ph += 2 * math.pi * (220 * 2 ** (x * 1.5)) / SR
        out.append(w[i] * 0.8 + 0.12 * x * x * math.sin(ph))
    return out


# ---------- ambient bed ----------

def midi(m):
    return 440 * 2 ** ((m - 69) / 12)


# One warm chord per scene, voiced low and soft.
CHORDS = {
    "open": [50, 57, 62, 66],      # D
    "plan": [50, 57, 62, 66, 69],  # D
    "screens": [47, 54, 59, 62, 66],  # Bm
    "slides": [43, 50, 55, 59, 62],   # G
    "migrate": [45, 52, 57, 61, 64],  # A
    "recap": [50, 57, 62, 66, 69],    # D
    "end": [43, 50, 59, 62, 67],      # Gmaj7-ish, resolves warm
}


def bed():
    order = list(SC.keys())
    xfade = 0.6
    phases = {}
    for i in range(N):
        t = i / SR
        if t > TOTAL:
            break
        v = 0.0
        for name in order:
            s0 = SC[name]["from"] / FPS
            s1 = s0 + SC[name]["dur"] / FPS
            if t < s0 - xfade or t > s1 + xfade:
                continue
            g = min(1.0, (t - (s0 - xfade)) / (2 * xfade), ((s1 + xfade) - t) / (2 * xfade))
            for k, m in enumerate(CHORDS[name]):
                f = midi(m) * (1 + 0.0015 * math.sin(2 * math.pi * 0.13 * t + k))
                key = (name, k)
                phases[key] = phases.get(key, k * 1.3) + 2 * math.pi * f / SR
                p = phases[key]
                # soft saw-ish tone: fundamental plus two weak harmonics
                v += g * (math.sin(p) + 0.18 * math.sin(2 * p) + 0.06 * math.sin(3 * p)) / (1 + k * 0.35)
        swell = 0.75 + 0.25 * math.sin(2 * math.pi * 0.08 * t)
        fade = min(1.0, t / 2.5, max(0.0, (TOTAL - t) / 2.5))
        mix[i] += v * 0.028 * swell * fade


# ---------- cue sheet ----------

def cues():
    e = EV["open"]
    for k, f in enumerate(e["ticks"]):
        add(at("open", f), tick(2600 if k % 2 else 2200), 0.35)
    add(at("open", e["line2"]), whoosh(0.7, 200, 2000), 0.25)
    add(at("open", e["line3"]), whoosh(0.6, 300, 2600), 0.22)
    add(at("plan", 0) - 1.0, riser(1.0), 0.35)

    e = EV["plan"]
    add(at("plan", 2), whoosh(0.5, 400, 4000), 0.3)
    for k, f in enumerate(e["items"]):
        add(at("plan", f), pop(480 + 60 * k), 0.45)
        add(at("plan", f) + 0.02, whoosh(0.25, 1500, 5000, 0.3), 0.12)
    add(at("plan", e["sync"]), chime(1046.5), 0.4)
    add(at("plan", e["sync"]) + 0.12, chime(1568), 0.22)

    e = EV["screens"]
    add(at("screens", 0) - 0.15, whoosh(0.6, 300, 3500), 0.32)
    add(at("screens", e["cropLabel"] - 4), thud(), 0.45)
    add(at("screens", e["calib"]), blip(600, 1200), 0.3)
    add(at("screens", e["calib"]) + 0.09, blip(900, 1800), 0.22)
    for f in e["edges"]:
        add(at("screens", f), whoosh(e["edgeDur"] / FPS, 800, 2500, 0.4), 0.22)
        add(at("screens", f + e["edgeDur"]), tick(1800), 0.25)
    add(at("screens", e["done"]), chime(880), 0.4)
    add(at("screens", e["done"]) + 0.1, chime(1318.5), 0.25)

    e = EV["slides"]
    add(at("slides", 0) - 0.15, whoosh(0.6, 300, 3500), 0.32)
    for k, f in enumerate(e["chips"]):
        add(at("slides", f), pop(600 + 120 * k), 0.4)
    for f in e["builds"]:
        add(at("slides", f), click(), 0.55)
        add(at("slides", f) + 0.01, whoosh(0.35, 600, 4000, 0.25), 0.18)
    add(at("slides", e["video"]), blip(500, 1000, 0.18), 0.28)
    add(at("slides", e["notes"]), pop(700), 0.3)
    add(at("slides", e["badge"]), chime(1174.7), 0.38)

    e = EV["migrate"]
    add(at("migrate", 0) - 0.15, whoosh(0.6, 300, 3500), 0.32)
    for k, f in enumerate(e["sources"]):
        add(at("migrate", f), pop(420 + 50 * k), 0.38)
    for k in range(len(e["sources"])):
        add(at("migrate", e["fly"] + k * e["flyStep"]), whoosh(0.45, 600, 5000, 0.5), 0.2)
    span = e["countTo"] - e["countFrom"]
    for f in range(e["countFrom"], e["countTo"], 2):
        x = (f - e["countFrom"]) / span
        add(at("migrate", f), tick(1400 + 1800 * x, 0.025), 0.16 + 0.1 * x)
    add(at("migrate", e["countTo"]), chime(987.8), 0.42)
    for k, f in enumerate(e["checks"]):
        add(at("migrate", f), pop(660 + 90 * k), 0.35)

    e = EV["recap"]
    add(at("recap", 0) - 0.2, riser(0.7), 0.25)
    add(at("recap", 4), whoosh(0.7, 200, 2500), 0.25)
    for k, f in enumerate(e["tiles"]):
        add(at("recap", f), pop([587, 659, 740, 880][k]), 0.42)

    e = EV["end"]
    add(at("end", e["mark"]), boom(), 0.5)
    add(at("end", e["mark"]), chime(587.3, 3.5), 0.45)
    add(at("end", e["mark"]) + 0.15, chime(880, 3.0), 0.3)
    add(at("end", e["word"]), whoosh(0.8, 200, 1800), 0.2)
    add(at("end", e["tag"]), chime(1174.7, 2.5), 0.2)


def main():
    bed()
    cues()
    peak = max(abs(v) for v in mix) or 1.0
    drive = 1.4
    norm = math.tanh(drive)
    frames = bytearray()
    for v in mix[: int(TOTAL * SR)]:
        s = math.tanh(drive * v / peak) / norm * 0.89  # soft limit to about -1 dBFS
        x = int(max(-1.0, min(1.0, s)) * 32767)
        frames += struct.pack("<hh", x, x)
    out = ROOT / "public" / "sfx.wav"
    out.parent.mkdir(exist_ok=True)
    with wave.open(str(out), "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(bytes(frames))
    print(f"wrote {out} ({TOTAL:.1f}s, peak before limit {peak:.2f})")


if __name__ == "__main__":
    main()
