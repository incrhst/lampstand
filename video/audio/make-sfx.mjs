// Synthesises the film's sound effects from scratch (noise, sines, envelopes),
// so there is nothing to license. Writes mono 16-bit WAVs to public/sfx.
import { writeFileSync, mkdirSync } from "node:fs";
const SR = 44100;
const out = new URL("../public/sfx/", import.meta.url).pathname;
mkdirSync(out, { recursive: true });

let seed = 7;
const rnd = () => ((seed = (seed * 1664525 + 1013904223) >>> 0) / 4294967296) * 2 - 1;
const wav = (name, samples) => {
  const peak = Math.max(...samples.map(Math.abs)) || 1;
  const buf = Buffer.alloc(44 + samples.length * 2);
  buf.write("RIFF", 0); buf.writeUInt32LE(36 + samples.length * 2, 4); buf.write("WAVEfmt ", 8);
  buf.writeUInt32LE(16, 16); buf.writeUInt16LE(1, 20); buf.writeUInt16LE(1, 22); buf.writeUInt32LE(SR, 24);
  buf.writeUInt32LE(SR * 2, 28); buf.writeUInt16LE(2, 32); buf.writeUInt16LE(16, 34); buf.write("data", 36);
  buf.writeUInt32LE(samples.length * 2, 40);
  samples.forEach((s, i) => buf.writeInt16LE(Math.round((s / peak) * 0.9 * 32767), 44 + i * 2));
  writeFileSync(out + name + ".wav", buf);
};
const gen = (secs, f) => Array.from({ length: Math.floor(secs * SR) }, (_, i) => f(i / SR, i));

// One-pole filters.
const lowpass = (xs, hz) => { const a = 1 - Math.exp((-2 * Math.PI * hz) / SR); let y = 0; return xs.map((x) => (y += a * (x - y))); };
const highpass = (xs, hz) => { const lp = lowpass(xs, hz); return xs.map((x, i) => x - lp[i]); };

// Whoosh: noise whose brightness and loudness swell and fade, for a camera glide.
{
  const d = 1.8; const n = gen(d, () => rnd());
  let y1 = 0, y2 = 0;
  const s = n.map((x, i) => {
    const t = i / SR / d;
    const cutoff = 300 + 2600 * Math.sin(Math.PI * t) ** 2;
    const a = 1 - Math.exp((-2 * Math.PI * cutoff) / SR);
    y1 += a * (x - y1); y2 += a * (y1 - y2);
    return y2 * Math.sin(Math.PI * t) ** 2.2;
  });
  wav("whoosh", s);
}
// Stamp: a rubber stamp thump with a little paper rustle, for each caption card.
{
  const body = gen(0.35, (t) => Math.sin(2 * Math.PI * (95 - 40 * t) * t) * Math.exp(-t * 18));
  const rustle = lowpass(highpass(gen(0.35, () => rnd()), 1200), 5000).map((x, i) => x * Math.exp(-(i / SR) * 30) * 0.5);
  wav("stamp", body.map((x, i) => x + rustle[i]));
}
// Key: a short mechanical click.
{
  const click = gen(0.12, (t) => rnd() * Math.exp(-t * 90));
  const tone = gen(0.12, (t) => Math.sin(2 * Math.PI * 1800 * t) * Math.exp(-t * 70) * 0.6);
  const f = highpass(click, 800);
  wav("key", f.map((x, i) => x + tone[i]));
}
// Bell: a soft typewriter-style ding for the end card.
{
  const parts = [[880, 1], [2217, 0.35], [3520, 0.18]];
  wav("bell", gen(2.4, (t) => parts.reduce((a, [f, g]) => a + g * Math.sin(2 * Math.PI * f * t) * Math.exp(-t * (2.2 + f / 1500)), 0) * Math.min(1, t * 400)));
}
console.log("wrote sfx to", out);
