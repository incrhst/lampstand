// Writes site/docs/watch.html from the scripts, so each clip's transcript on
// the page is the text the narrator read.
//   node narration/watch-page.mjs
import { readFileSync, writeFileSync } from "node:fs";
import { execFileSync } from "node:child_process";

const HERE = new URL("./", import.meta.url).pathname;
const PAGE = new URL("../../site/docs/watch.html", import.meta.url).pathname;
const esc = (s) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/&amp;#8217;/g, "&rsquo;");
const rsquo = (s) => esc(s).replace(/'/g, "&rsquo;");

// title, one line for the card, and the docs section the clip belongs to.
const groups = [
  { h: "Before you start", id: "before", clips: [
    ["01-why", "Why one laptop", "What Lampstand puts in one place.", "../#get"],
    ["02-install", "Install it", "Mac, Windows and Linux.", "getting-started.html#install"],
  ] },
  { h: "Your first service", id: "first", clips: [
    ["03-welcome", "Try the Welcome service", "Go live with Space, step back, blank the screen.", "getting-started.html#first"],
    ["04-screens", "Choose and calibrate your screen", "So words never run off the edge.", "getting-started.html#first"],
    ["05-songs", "Bring your songs across", "Seven formats, and a report of what did not.", "using.html#songs"],
    ["06-plan", "Plan the order of service", "Quick add a song or a verse, then open it for live.", "using.html#modes"],
  ] },
  { h: "On service day", id: "service", clips: [
    ["07-slides", "PowerPoint and Google Slides", "Decks play without PowerPoint on the laptop.", "using.html#slides"],
    ["08-present", "Run the service", "Keys, verses, blank, and speaker notes.", "using.html#keys"],
    ["09-loop", "Loop the pre-service slides", "A group that advances by itself.", "using.html#groups"],
  ] },
  { h: "Make it yours, and beyond one laptop", id: "beyond", clips: [
    ["10-look", "Your church&rsquo;s look", "Name, logo, colours, font and the logo bug.", "using.html#brand"],
    ["11-stream", "Feed your live stream", "Lyrics to OBS as a browser source.", "using.html#obs"],
    ["12-sync", "Plan together during the week", "Pair a device with a six-digit code.", "how-it-works.html#sync"],
    ["13-sunday", "Sunday morning, offline", "Saved as you go, safe updates, backups.", "getting-started.html#data"],
  ] },
];

const secs = (id) => Math.round(Number(execFileSync("ffprobe", ["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", `${HERE}out/clips/${id}.mp4`], { encoding: "utf8" }))) ;

const clip = ([id, title, line, link]) => {
  const text = readFileSync(`${HERE}scripts/${id}.txt`, "utf8").trim();
  return `<figure class="narrated" id="${id}">
  <h3>${title} <span class="len">${secs(id)}&nbsp;s</span></h3>
  <video controls preload="none" playsinline width="1280" height="800" poster="clips/narrated/${id}.jpg" aria-label="${rsquo(title.replace(/&rsquo;/g, "'"))}: ${rsquo(line)}">
    <source src="clips/narrated/${id}.mp4" type="video/mp4">
    <track kind="captions" src="clips/narrated/${id}.vtt" srclang="en" label="English" default>
  </video>
  <figcaption>${line} <a href="${link}">Read the docs</a>.
    <details><summary>Transcript</summary><p>${rsquo(text)}</p></details></figcaption>
</figure>`;
};

const nav = `<a href="./">Docs</a><a href="getting-started.html">Getting started</a><a href="using.html">Using Lampstand</a><a href="watch.html" aria-current="page">Watch</a><a href="how-it-works.html">How it works</a><a href="contributing.html">Contributing</a><a href="decisions.html">Decisions</a>`;
const toc = groups.map((g) => `<li><a href="#${g.id}">${g.h}</a></li>`).join("");
const body = groups.map((g) => `<h2 id="${g.id}">${g.h}</h2>\n${g.clips.map(clip).join("\n")}`).join("\n");

writeFileSync(PAGE, `<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Watch · Lampstand docs</title>
  <meta name="description" content="Thirteen short narrated clips, each under 30 seconds, that walk through Lampstand from download to Sunday morning.">
  <meta name="theme-color" content="#2b211a">
  <link rel="icon" href="../favicon.svg" type="image/svg+xml">
  <link rel="stylesheet" href="../style.css">
  <link rel="stylesheet" href="docs.css">
</head>
<body>
  <main class="sheet sheet--docs">
    <header class="docs-bar">
      <a class="brand" href="../">Lampstand</a>
      <nav aria-label="Documentation">${nav}</nav>
    </header>
    <div class="docs-wrap">
      <aside class="toc" aria-label="On this page"><h2>On this page</h2><ol>${toc}</ol></aside>
      <article class="doc">
        <h1>Watch</h1>
        <p class="lede">Thirteen short clips, each under 30 seconds, with a narrator. They follow the order a church meets Lampstand in, from download to Sunday morning. Every clip has captions and a transcript.</p>
${body}
        <nav class="pager" aria-label="Pages"><span>&larr; <a href="using.html">Using Lampstand</a></span><span><a href="how-it-works.html">How it works</a> &rarr;</span></nav>
      </article>
    </div>
    <footer class="colophon typed">
      <p>Lampstand is made in the open at <a href="https://github.com/incrhst/lampstand">github.com/incrhst/lampstand</a> &middot; GPL-3.0-or-later &middot; The narrator is a synthetic voice, and the clips show the app running on sample songs.</p>
    </footer>
  </main>
</body>
</html>
`);
console.log("wrote", PAGE);
