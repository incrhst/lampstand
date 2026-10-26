import { emit } from "./mock-event";

const S = (title: string, sections: [string, string[]][], extra: object = {}) => ({
  id: title.toLowerCase().replace(/\W+/g, "-"), title, author: null, copyright: null, ccli_number: null, key: null, tempo: null,
  sections: sections.map(([label, lines]) => ({ label, lines })), arrangement: [], ...extra,
});
const songs = [
  S("Amazing Grace", [["V1", ["Amazing grace, how sweet the sound", "That saved a wretch like me"]], ["V2", ["'Twas grace that taught my heart to fear", "And grace my fears relieved"]], ["V3", ["Through many dangers, toils and snares", "I have already come"]]], { author: "John Newton", key: "G" }),
  S("How Great Thou Art", [["V1", ["O Lord my God, when I in awesome wonder", "Consider all the worlds thy hands have made"]], ["C", ["Then sings my soul, my Saviour God, to thee", "How great thou art, how great thou art"]]], { author: "Stuart K. Hine", key: "Bb" }),
  S("Be Thou My Vision", [["V1", ["Be thou my vision, O Lord of my heart", "Naught be all else to me, save that thou art"]]], { author: "Ancient Irish" }),
  S("Great Is Thy Faithfulness", [["V1", ["Great is thy faithfulness, O God my Father", "There is no shadow of turning with thee"]], ["C", ["Great is thy faithfulness", "Morning by morning new mercies I see"]]], { author: "Thomas Chisholm" }),
  S("It Is Well", [["V1", ["When peace like a river attendeth my way", "When sorrows like sea billows roll"]]], { author: "Horatio Spafford" }),
  S("Holy, Holy, Holy", [["V1", ["Holy, holy, holy! Lord God Almighty!", "Early in the morning our song shall rise to thee"]]], { author: "Reginald Heber" }),
  S("Come, Thou Fount", [["V1", ["Come, thou Fount of every blessing", "Tune my heart to sing thy grace"]]], { author: "Robert Robinson" }),
  S("Blessed Be Your Name", [["V1", ["Blessed be your name in the land that is plentiful", "Where your streams of abundance flow"]]], { author: "Matt Redman" }),
];
const verses: Record<string, [number, number, string][]> = {
  "John 3:16": [[3, 16, "For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life."]],
  "Romans 8:28": [[8, 28, "And we know that all things work together for good to them that love God, to them who are the called according to his purpose."]],
  "Psalm 23:1": [[23, 1, "The Lord is my shepherd; I shall not want."], [23, 2, "He maketh me to lie down in green pastures: he leadeth me beside the still waters."]],
};
const decks = [{ id: "deck-announce", title: "Sunday Announcements.pptx", source: { File: { name: "Sunday Announcements.pptx" } }, current: { blob: "d1", fetched_at: 0, source_version: null }, pending: null },
  { id: "deck-sermon", title: "Sermon: The Good Shepherd", source: { GoogleSlidesLink: { url: "https://docs.google.com/presentation/d/x" } }, current: { blob: "d2", fetched_at: 0, source_version: null }, pending: null }];
const deckSlides: Record<string, any[]> = {
  "deck-announce": [
    { label: "Slide 1", lines: ["Welcome", "Coffee after the service in the hall"], steps: 2, notes: "Mention the coffee rota.\nThank the welcome team." },
    { label: "Slide 2", lines: ["This week", "Youth night Friday 7pm"], steps: 3, notes: "Remind parents about pick-up." },
  ],
  "deck-sermon": [
    { label: "Slide 1", lines: ["The Good Shepherd", "John 10"], steps: 0, notes: "Open with the story of the lost sheep." },
    { label: "Slide 2", lines: ["I know my sheep", "They hear my voice", "They follow me"], steps: 2, notes: "Pause here. Ask: who knows your name?" },
    { label: "Slide 3", lines: ["He lays down his life"], steps: 1, notes: "Slow down." },
  ],
};
// The training services' decks (#190), with the slides and notes of training/decks.
const trainingDecks = [
  { id: "deck-t1-screen", title: "The Present screen", source: { File: { name: "t1-present-screen.pptx" } }, current: { blob: "t1", fetched_at: 0, source_version: null }, pending: null },
  { id: "deck-t1-sermon", title: "Practice: a sermon deck", source: { File: { name: "t1-sermon-practice.pptx" } }, current: { blob: "t2", fetched_at: 0, source_version: null }, pending: null },
  { id: "deck-t2-thursday", title: "Sermon (sent on Thursday)", source: { File: { name: "t2-sermon-thursday.pptx" } }, current: { blob: "t3", fetched_at: 0, source_version: null }, pending: null },
  { id: "deck-t2-sunday", title: "Sermon (updated Sunday morning)", source: { File: { name: "t2-sermon-sunday.pptx" } }, current: { blob: "t4", fetched_at: 0, source_version: null }, pending: null },
];
deckSlides["deck-t1-screen"] = [
  { label: "Slide 1", lines: ["The Present screen", "Four places to look. One rule."], steps: 0, notes: "Trainer: point at each part of the screen as we go." },
  { label: "Slide 2", lines: ["Four places to look"], steps: 4, notes: "The wall is every slide. Red LIVE is on air; gold NEXT is the next slide. The strip on top is the main screen and the controls." },
];
deckSlides["deck-t1-sermon"] = [
  { label: "Slide 1", lines: ["The Lord is my shepherd", "Psalm 23"], steps: 0, notes: "Preacher: Good morning. Our reading today is Psalm 23.\nOperator: each Space press brings in one point." },
  { label: "Slide 2", lines: ["He leads me", "Green pastures", "Still waters", "Paths of righteousness"], steps: 3, notes: "Preacher: He leads me beside the still waters.\nOperator: three points, three presses." },
  { label: "Slide 3", lines: ["Yea, though I walk through the valley", "of the shadow of death, I will fear no evil"], steps: 0, notes: "Preacher: read the verse slowly." },
  { label: "Slide 4", lines: ["Three things to take home", "You are known", "You are led", "You are not alone"], steps: 3, notes: "Preacher: three things to take home this week." },
  { label: "Slide 5", lines: ["Surely goodness and mercy", "shall follow me all the days of my life"], steps: 0, notes: "Preacher: let us pray." },
];
deckSlides["deck-t2-thursday"] = [{ label: "Slide 1", lines: ["The Lord is my shepherd", "As sent on Thursday"], steps: 0, notes: "SURPRISE 4 (updated deck). Trainer: \"The preacher changed the verse slide this morning. Use the new deck.\"" }];
deckSlides["deck-t2-sunday"] = [{ label: "Slide 1", lines: ["The Lord is my shepherd", "Updated Sunday morning"], steps: 0, notes: "Preacher: Good morning. This is the corrected deck." }];
decks.push(...trainingDecks);
let seq = 0;
const pos = () => String(1000 + seq++);
const item = (kind: any, notes = "") => ({ position: pos(), kind, notes, obs_scene: null, loop_seconds: null });
const sunday = {
  id: "sunday", title: "Sunday service", starts_at: new Date("2026-10-25T10:00:00").getTime(), time_zone: null, theme: null,
  items: {
    i1: item({ Header: { title: "Welcome" } }),
    i2: item({ Deck: { deck: "deck-announce" } }, "Coffee rota, youth night."),
    i3: item({ Song: { song: "amazing-grace", arrangement: null } }),
    i4: item({ Scripture: { passage: { book: 43, start: { chapter: 3, verse: 16 }, end: { chapter: 3, verse: 16 } }, translation: "kjv" } }),
    i5: item({ Song: { song: "how-great-thou-art", arrangement: null } }),
    i6: item({ Deck: { deck: "deck-sermon" } }, "Sermon starts here."),
  } as Record<string, any>,
};
const welcome = {
  id: "welcome", title: "Welcome to Lampstand", starts_at: null, time_zone: null, theme: null,
  items: { w1: item({ Song: { song: "amazing-grace", arrangement: null } }), w2: item({ Scripture: { passage: { book: 43, start: { chapter: 3, verse: 16 }, end: { chapter: 3, verse: 16 } }, translation: "kjv" } }), w3: item({ Song: { song: "how-great-thou-art", arrangement: null } }) } as Record<string, any>,
};
const tipsService = {
  id: "tips", title: "Pre-service and service", starts_at: null, time_zone: null, theme: null,
  items: {
    t1: { ...item({ Header: { title: "Pre-service" } }), loop_seconds: (window as any).__scene === "tips-loop" ? 3 : null },
    t2: item({ Song: { song: "holy-holy-holy", arrangement: null } }),
    t3: item({ Song: { song: "amazing-grace", arrangement: null } }),
    t4: item({ Header: { title: "Service" } }),
    t5: item({ Song: { song: "be-thou-my-vision", arrangement: null } }),
    t6: item({ Song: { song: "it-is-well", arrangement: null } }),
  } as Record<string, any>,
};
const john = { book: 43, start: { chapter: 3, verse: 16 }, end: { chapter: 3, verse: 18 } };
const psalm = { book: 19, start: { chapter: 23, verse: 1 }, end: { chapter: 23, verse: 3 } };
const training = [
  { id: "training-0", title: "Training 0: Orientation", training: true, starts_at: null, time_zone: null, theme: null, items: {
    a1: item({ Header: { title: "Training 0: Orientation (5 minutes). Press Space to start" } }),
    a2: item({ Media: { blob: "v0", name: "Welcome to Lampstand.mp4", looped: false } }),
    a3: item({ Deck: { deck: "deck-t1-screen" } }),
  } as Record<string, any> },
  { id: "training-1", title: "Training 1: Run a service", training: true, starts_at: null, time_zone: null, theme: null, items: {
    b1: item({ Header: { title: "Welcome. Press Space to start" } }),
    b2: item({ Media: { blob: "v1", name: "Welcome.mp4", looped: false } }, "Press Space when the video ends."),
    b3: item({ Deck: { deck: "deck-t1-screen" } }, "Point at each part of the real screen as the slides describe it."),
    b4: item({ Media: { blob: "v2", name: "Going live and moving through slides.mp4", looped: false } }),
    b5: item({ Header: { title: "Practice. Run these as if it were Sunday" } }),
    b6: item({ Song: { song: "amazing-grace", arrangement: null } }, "Practice: press Space for each verse. At the last verse, press 1 to jump back to verse 1."),
    b7: item({ Scripture: { passage: john, translation: "kjv" } }, "Practice: a Bible reading. Space through the verses."),
    b8: item({ Media: { blob: "v3", name: "Blank, Idle, Clear.mp4", looped: false } }),
    b9: item({ Deck: { deck: "deck-t1-sermon" } }, "Practice: five slides with builds. Try Blank (full stop) once in the middle."),
  } as Record<string, any> },
  { id: "training-2", title: "Training 2: Confident operator", training: true, starts_at: null, time_zone: null, theme: null, items: {
    c1: item({ Header: { title: "Rehearsal service. The trainer calls the surprises" } }),
    c2: item({ Text: { title: "Welcome", body: "Welcome!\nWe are glad you are here.", attribution: null } }, "SURPRISE 5 (blank in an emergency) can come at any moment from here on."),
    c3: item({ Song: { song: "holy-holy-holy", arrangement: null } }, "SURPRISE 2 (order change). Trainer: \"We are singing Amazing Grace first.\""),
    c4: item({ Song: { song: "amazing-grace", arrangement: null } }, "SURPRISE 1 (repeated verse). Trainer, during the last verse: \"Again! Verse one!\" Operator: press 1."),
    c5: item({ Scripture: { passage: psalm, translation: "kjv" } }, "SURPRISE 3 (scripture on the fly). Trainer: \"Turn with me to Romans 8:28.\" Operator: press /, type Rom 8:28, then Shift+Enter."),
    c6: item({ Deck: { deck: "deck-t2-thursday" } }, "SURPRISE 4 (updated deck). The trainee should skip this deck and use the next one."),
    c7: item({ Deck: { deck: "deck-t2-sunday" } }, "The corrected deck. Space through the builds."),
  } as Record<string, any> },
];
const STARTER = "01920000-0000-7000-8000-6c616d707374";
const themes: any[] = [{
  id: STARTER, name: "Starter",
  text: { font_family: "Noto Sans", size: 72, min_size: 36, weight: 400, italic: false, color: { r: 255, g: 255, b: 255, a: 255 }, align: "Center", valign: "Middle", line_spacing: 1.2, uppercase: false, shadow: null, outline: null },
  background: { Color: { r: 0, g: 0, b: 0, a: 255 } }, text_area: { x: 0, y: 0, width: 1, height: 1 }, max_lines: 4, transition: { kind: "Cut", duration_ms: 0 }, show_copyright: true,
}];
let settings: any = { clicker_mode: false, default_translation: null, brand: null, brand_theme: null, default_theme: null, idle: null, church_synced: false };
const hex = (c: any) => "#" + [c.r, c.g, c.b].map((n: number) => n.toString(16).padStart(2, "0")).join("");
// A stand-in for the picture the core draws of a theme: its colours and font on a 16:9 card.
const themeThumb = (t: any, sample: string, w: number, h: number) => {
  const lines = sample === "Verse" ? ["For God so loved the world, that he gave", "his only begotten Son"] : sample === "Text" ? ["Welcome", "Coffee after the service"] : ["Amazing grace! How sweet the sound", "That saved a wretch like me!"];
  const bg = t.background?.Color ? hex(t.background.Color) : "#000";
  const size = Math.round(h * (t.text.size / 1080));
  const y0 = t.text_area.y * h + (t.text_area.height * h - lines.length * size * 1.2) / 2 + size;
  const x = (t.text_area.x + t.text_area.width / 2) * w;
  const t0 = lines.map((l, i) => `<text x='${x}' y='${y0 + i * size * 1.2}' fill='${hex(t.text.color)}' font-family='${t.text.font_family}, sans-serif' font-size='${size}' font-weight='${t.text.weight}' text-anchor='middle'>${l}</text>`).join("");
  return "data:image/svg+xml;utf8," + encodeURIComponent(`<svg xmlns='http://www.w3.org/2000/svg' width='${w}' height='${h}'><rect width='${w}' height='${h}' fill='${bg}'/>${t0}</svg>`);
};
const state: any = { services: String((window as any).__scene).startsWith("tips") ? [tipsService] : String((window as any).__scene).startsWith("training") ? [welcome, sunday, ...training] : [welcome, sunday], outputs: [], loaded: null, loopOverrides: {} as Record<string, number | null>, live: null, preview: null, overlay: "None", obs: null, scene: (window as any).__scene ?? "default" };
if (state.scene !== "fresh") state.outputs.push({ id: "out1", name: "Sanctuary projector", role: { Audience: { primary: true } }, target: { Window: { width: 1920, height: 1080 } }, safe_area: { top: 0.03, right: 0.03, bottom: 0.03, left: 0.03 }, background: "Bleed", theme: null, idle: null, enabled: true });
(window as any).__mock = state;

const ID = (n: string) => n;
const sorted = (s: any) => Object.entries(s.items).sort((a: any, b: any) => a[1].position.localeCompare(b[1].position));
function slidesFor(kind: any) {
  if (kind.Header) return [];
  if (kind.Song) { const s = songs.find((x) => x.id === kind.Song.song)!; return s.sections.map((sec) => ({ label: sec.label, lines: sec.lines, steps: 0, notes: "" })); }
  if (kind.Scripture?.passage.book === 19) return verses["Psalm 23:1"].map(([c, v, t]) => ({ label: `Psalm ${c}:${v}`, lines: [t], steps: 0, notes: "" }));
  if (kind.Scripture?.passage.book === 45) return [{ label: "Romans 8:28", lines: [verses["Romans 8:28"][0][2]], steps: 0, notes: "" }];
  if (kind.Scripture) return [{ label: "John 3:16", lines: [verses["John 3:16"][0][2]], steps: 0, notes: "" }];
  // As the engine: a deck's slides are pictures (no lines), drawn by SlideThumbnail at their build step.
  if (kind.Deck) return (deckSlides[kind.Deck.deck] ?? []).map((sl) => ({ ...sl, lines: [] }));
  if (kind.Media) return [{ label: "Video", lines: [], steps: 0, notes: "" }];
  if (kind.Text) {
    const groups: string[][] = [];
    let cur: string[] = [];
    for (const raw of kind.Text.body.split(/\r?\n/)) {
      const line = raw.trim();
      if (line === "" || /^-{3,}$/.test(line)) { if (cur.length) groups.push(cur); cur = []; } else cur.push(line);
    }
    if (cur.length) groups.push(cur);
    return groups.map((lines, i) => ({ label: groups.length > 1 ? `${i + 1}/${groups.length}` : "Text", lines, steps: 0, notes: "" }));
  }
  return [];
}
function titleFor(kind: any) {
  if (kind.Header) return kind.Header.title;
  if (kind.Song) return songs.find((x) => x.id === kind.Song.song)!.title;
  if (kind.Scripture?.passage.book === 19) return "Psalm 23:1-3";
  if (kind.Scripture?.passage.book === 45) return "Romans 8:28";
  if (kind.Scripture?.passage.end?.verse === 18) return "John 3:16-18";
  if (kind.Scripture) return "John 3:16";
  if (kind.Media) return kind.Media.name.replace(/\.mp4$/, "");
  if (kind.Deck) return decks.find((d) => d.id === kind.Deck.deck)!.title;
  if (kind.Text) return kind.Text.title.trim() || "Text";
  return "Item";
}
function load(id: string) {
  const s = state.services.find((x: any) => x.id === id);
  const sections: any[] = [];
  for (const [iid, it] of sorted(s) as any) {
    if (it.kind.Header) {
      const own = state.loopOverrides[iid];
      const seconds = own !== undefined ? own : it.loop_seconds ?? null;
      sections.push({ header: iid, title: it.kind.Header.title, seconds, items: [] });
    } else if (sections.length) sections[sections.length - 1].items.push(iid);
  }
  return { service: s.id, title: s.title, items: sorted(s).map(([iid, it]: any) => ({ id: iid, title: titleFor(it.kind), slides: slidesFor(it.kind) })), sections };
}
let liveKey = "";
let liveSince = 0;
function pushLive() {
  const key = state.live ? `${state.live.item}:${state.live.slide}` : "";
  if (key !== liveKey) { liveKey = key; liveSince = Date.now(); }
  emit({ type: "Live", data: { live: state.live, preview: state.preview, idle: !state.live && state.overlay === "None", overlay: state.overlay } });
  if (soundOn) emit({ type: "Sound", data: soundView() });
}
// Sound (window.__soundOn): Amazing Grace has a backing track that starts on its own when the song
// goes live, as Next does in the engine, and plays on until another sound starts.
let soundOn = false;
let playing: any = null;
const BACKING = { name: "Amazing Grace (backing track)", file: "s1", length_ms: 212000, start: "OnItsOwn", keep_off_stream: false };
function soundView() {
  if (state.live?.item === "i3" && playing?.item !== "i3") playing = { item: "i3", name: BACKING.name, paused: false, ending: false, position_ms: 4000, length_ms: BACKING.length_ms };
  return { here: true, holder: "Demo laptop", away: false, nobody: false, output_ok: true, playing, problem: null, problem_item: null };
}
// A looping section moves the live slide on by itself, as the real engine does.
setInterval(() => {
  if (!state.live || !state.loaded) return;
  const sec = state.loaded.sections.find((x: any) => x.items.includes(state.live.item));
  if (!sec?.seconds || Date.now() - liveSince < sec.seconds * 1000) return;
  const cursors = sec.items.flatMap((id: string) => state.loaded.items.find((i: any) => i.id === id).slides.map((_: any, n: number) => ({ item: id, slide: n, step: 0 })));
  const at = cursors.findIndex((c: any) => c.item === state.live.item && c.slide === state.live.slide);
  state.live = state.preview = cursors[(at + 1) % cursors.length];
  pushLive();
}, 200);
const flat = () => (state.loaded ? state.loaded.items.flatMap((i: any) => i.slides.map((_: any, n: number) => ({ item: i.id, slide: n, step: 0 }))) : []);
function move(dir: number, byItem = false) {
  const f = flat(); if (!f.length) return;
  // As the engine: Next plays a slide's builds one click at a time before moving on.
  const cur = state.live;
  if (!byItem && cur) {
    const steps = state.loaded.items.find((i: any) => i.id === cur.item)?.slides[cur.slide]?.steps ?? 0;
    const step = cur.step + dir;
    if (step >= 0 && step <= steps) { state.preview = { ...cur, step }; return; }
  }
  // Previous from a slide's first step lands on the slide before, fully built.
  const built = (c: any) => (dir < 0 && !byItem && c ? { ...c, step: state.loaded.items.find((i: any) => i.id === c.item)?.slides[c.slide]?.steps ?? 0 } : c);
  const base = state.live ?? state.preview; let at = base ? f.findIndex((c: any) => c.item === base.item && c.slide === base.slide) : -1;
  if (byItem) { const cur = base?.item; let n = at; while (n >= 0 && n < f.length && f[n].item === cur) n += dir; at = n < 0 ? 0 : Math.min(n, f.length - 1); if (dir < 0) { const it = f[at].item; at = f.findIndex((c: any) => c.item === it); } }
  else at = Math.max(0, Math.min(f.length - 1, at + dir));
  state.preview = built(f[Math.max(0, at)]);
}
const svgThumb = (label: string) => "data:image/svg+xml;utf8," + encodeURIComponent(`<svg xmlns='http://www.w3.org/2000/svg' width='320' height='180'><rect width='320' height='180' fill='#0b0e16'/><text x='160' y='95' fill='#f5f1e8' font-family='serif' font-size='20' text-anchor='middle'>${label}</text></svg>`);

/** A training deck's slide as the core draws it: the heading, then one point per build step. */
const esc = (t: string) => t.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/'/g, "&apos;");
function deckThumb(sl: any, step: number) {
  const [head, ...rest] = sl.lines as string[];
  const shown = sl.steps > 0 ? rest.slice(0, step) : rest;
  const body = shown.map((l, i) => `<text x='160' y='${92 + i * 20}' fill='#f5f1e8' font-family='sans-serif' font-size='14' text-anchor='middle'>${esc(l)}</text>`).join("");
  return "data:image/svg+xml;utf8," + encodeURIComponent(`<svg xmlns='http://www.w3.org/2000/svg' width='320' height='180'><rect width='320' height='180' fill='#0b0e16'/><text x='160' y='${rest.length ? 58 : 96}' fill='#e9b949' font-family='sans-serif' font-weight='700' font-size='${head.length > 30 ? 13 : 20}' text-anchor='middle'>${esc(head)}</text>${body}</svg>`);
}
const logoThumb = "data:image/svg+xml;utf8," + encodeURIComponent(`<svg xmlns='http://www.w3.org/2000/svg' width='320' height='180'><rect width='320' height='180' fill='#0b0e16'/><circle cx='160' cy='80' r='18' fill='none' stroke='#e9b949' stroke-width='3'/><path d='M160 68v24M150 78h20' stroke='#e9b949' stroke-width='3'/><text x='160' y='120' fill='#f5f1e8' font-family='serif' font-size='13' text-anchor='middle'>Grace Chapel</text></svg>`);

const summaries = () => state.services.map((s: any) => ({ id: s.id, title: s.title, starts_at: s.starts_at, time_zone: s.time_zone, items: Object.values(s.items).filter((i: any) => !i.kind.Header).length, training: !!s.training }));

function api(req: any): any {
  const { area, request: r } = req;
  const key = typeof r === "string" ? r : Object.keys(r)[0];
  const arg = typeof r === "string" ? undefined : r[key];
  const ok = { type: "Ok" };
  switch (area) {
    case "Library":
      switch (key) {
        case "SearchSongs": { const q = (arg.query || "").toLowerCase(); return { type: "Songs", data: (state.scene === "fresh" ? songs.slice(0, 2) : songs).filter((s) => s.title.toLowerCase().includes(q)).slice(0, arg.limit).map((s) => ({ song: s.id, title: s.title, line: s.sections[0].lines[0] })) }; }
        case "GetSong": return { type: "Song", data: songs.find((s) => s.id === arg) };
        case "ListThemes": return { type: "Themes", data: themes };
        case "PutTheme": { const i = themes.findIndex((t) => t.id === arg.id); if (i >= 0) themes[i] = arg; else themes.push(arg); emit({ type: "Changed", data: { kind: "theme", id: arg.id } }); return ok; }
        case "DeleteTheme": {
          if (arg === STARTER) return { type: "Error", data: { message: "The Starter theme can't be deleted: it is what every screen falls back to. Duplicate it to make your own." } };
          const i = themes.findIndex((t) => t.id === arg); if (i >= 0) themes.splice(i, 1);
          if (settings.default_theme === arg) settings = { ...settings, default_theme: null };
          return ok;
        }
        case "RestoreStarterTheme": return ok;
        case "ExportBrand": return { type: "Text", data: "Saved the brand: 1 theme and 1 picture or video file." };
        case "InspectBrand": return { type: "BrandFile", data: { name: "Grace Chapel", has_logo: true, font_family: "Noto Serif", font_installed: true, missing_fonts: [], themes: ["Grace Chapel"], idle: null, has_brand: true } };
        case "ImportBrand": return { type: "Text", data: "Imported the brand: 1 theme added, 0 replaced." };
        case "ThemePreview": return { type: "Thumbnail", data: themeThumb(arg.theme, arg.sample, arg.thumb_width, Math.round(arg.thumb_width * arg.height / arg.width)) };
        case "DeckSlidePicture": return { type: "DeckSlide", data: { thumbnail: svgThumb(`Slide ${arg.slide + 1}`), slides: (deckSlides[arg.deck] ?? []).length || 3 } };
        case "ListTranslations": return { type: "Translations", data: [{ id: "kjv", name: "King James Version", abbreviation: "KJV", language: "en", licence: "PublicDomain", text: "kjv" }] };
        case "LookupPassage": {
          const typed = arg.reference.toLowerCase();
          const ref = typed.startsWith("rom") ? "Romans 8:28" : Object.keys(verses).find((k) => typed.startsWith(k.toLowerCase().split(":")[0].toLowerCase())) ?? "John 3:16";
          const passage = ref === "Romans 8:28" ? { book: 45, start: { chapter: 8, verse: 28 }, end: { chapter: 8, verse: 28 } } : { book: 43, start: { chapter: 3, verse: 16 }, end: { chapter: 3, verse: 16 } };
          return { type: "Passage", data: { passage, display: ref, verses: verses[ref] } };
        }
        case "ListSongDatabases": return { type: "SongDatabases", data: [{ code: "en", name: "English", source: "Free song database" }] };
        case "ListDecks": return { type: "Decks", data: state.scene === "empty" ? [] : decks.filter((d) => !trainingDecks.includes(d)) };
        case "ListTrainingDecks": return { type: "Decks", data: trainingDecks };
        case "ListMedia": return { type: "MediaFiles", data: [] };
        case "DeckThumbnail": case "MediaThumbnail": return { type: "Thumbnail", data: svgThumb("Slides") };
        // With a logo, the default idle screen is the logo, small and centred.
        case "IdlePreview": return { type: "Thumbnail", data: logoThumb };
      }
      break;
    case "Service":
      switch (key) {
        case "List": return { type: "Services", data: summaries() };
        case "ListChecked": return { type: "ServiceList", data: { services: summaries(), newer: 0, damaged: [] } };
        case "ResetTraining": return ok;
        // The screenshots show a device that has its training videos.
        case "TrainingVideos": return { type: "TrainingVideos", data: { state: "Ready", message: "" } };
        case "Get": return { type: "Service", data: state.services.find((s: any) => s.id === arg) };
        case "Duplicate": { const src = state.services.find((x: any) => x.id === arg.service); if (!src) throw "no such service"; const items: any = {}; for (const it of Object.values(src.items)) items["itm" + Date.now() + Math.random().toString(36).slice(2)] = JSON.parse(JSON.stringify(it)); const s = { ...JSON.parse(JSON.stringify(src)), id: "svc" + Date.now(), title: arg.title, starts_at: arg.starts_at, time_zone: arg.time_zone, items }; state.services.push(s); return { type: "Service", data: s }; }
        case "Create": { const s = { id: "svc" + Date.now(), title: arg.title, starts_at: arg.starts_at, time_zone: null, theme: null, items: {} }; state.services.push(s); return { type: "Service", data: s }; }
        case "InsertItem": {
          const s = state.services.find((x: any) => x.id === arg.service);
          const id = "n" + Date.now() + (seq++);
          s.items[id] = item(arg.kind);
          const order = sorted(s).filter(([k]: any) => k !== id).map(([k]: any) => k);
          order.splice(Math.min(arg.index, order.length), 0, id);
          order.forEach((k: string, n: number) => (s.items[k].position = String(2000 + n).padStart(5, "0")));
          return { type: "Service", data: s };
        }
        case "MoveItem": {
          const s = state.services.find((x: any) => x.id === arg.service);
          const order = sorted(s).map(([k]: any) => k).filter((k: string) => k !== arg.item);
          order.splice(arg.index, 0, arg.item);
          order.forEach((k: string, n: number) => (s.items[k].position = String(2000 + n).padStart(5, "0")));
          return { type: "Service", data: s };
        }
        case "RemoveItem": { const s = state.services.find((x: any) => x.id === arg.service); delete s.items[arg.item]; return { type: "Service", data: s }; }
        case "SetItemText": {
          const s = state.services.find((x: any) => x.id === arg.service);
          const k = s.items[arg.item].kind.Text;
          k.title = arg.title; k.body = arg.body; k.attribution = arg.attribution?.trim() || null;
          if (state.loaded?.service === s.id) { state.loaded = load(s.id); emit({ type: "Loaded", data: state.loaded }); }
          return { type: "Service", data: s };
        }
        case "SetLoop": { const s = state.services.find((x: any) => x.id === arg.service); s.items[arg.item].loop_seconds = arg.seconds; return { type: "Service", data: s }; }
        case "SetMediaLooped": { const s = state.services.find((x: any) => x.id === arg.service); s.items[arg.item].kind.Media.looped = arg.looped; return { type: "Service", data: s }; }
        case "Close": { state.loaded = null; state.live = null; state.preview = null; state.loopOverrides = {}; pushLive(); return ok; }
        case "SetObsScene": case "Reschedule": return ok;
        case "SetTheme": { const s = state.services.find((x: any) => x.id === arg.service); s.theme = arg.theme; return { type: "Service", data: s }; }
        case "SetLiveLoop": {
          state.loopOverrides[arg.header] = arg.seconds;
          if (state.loaded) { state.loaded = load(state.loaded.service); emit({ type: "Loaded", data: state.loaded }); }
          return ok;
        }
        case "Estimate": {
          const s = state.services.find((x: any) => x.id === arg);
          return { type: "Estimate", data: sorted(s).filter(([, it]: any) => !it.kind.Header).map(([iid, it]: any) => ({
            item: iid,
            stills: it.kind.Media ? 0 : slidesFor(it.kind).length,
            click_slides: slidesFor(it.kind).filter((sl: any) => sl.steps > 0).length,
            video_ms: it.kind.Media ? 45000 : null,
            self_loop: !!it.kind.Media?.looped,
            open_ended: !!it.kind.Countdown,
          })) };
        }
        case "Open": { state.loaded = load(arg); emit({ type: "Loaded", data: state.loaded }); return { type: "Loaded", data: state.loaded }; }
        case "SlideThumbnail": {
          const kind = state.services.flatMap((sv: any) => Object.entries(sv.items)).find(([id]: any) => id === arg.item)?.[1]?.kind;
          const sl = kind?.Deck ? deckSlides[kind.Deck.deck]?.[arg.slide] : null;
          return { type: "Thumbnail", data: sl ? deckThumb(sl, arg.step) : svgThumb("Slide") };
        }
        case "Readiness": return { type: "Text", data: "ready" };
      }
      break;
    case "Live":
      // As the engine: a move that takes a slide live leaves the idle screen.
      if (state.overlay === "Idle" && ["Next", "Previous", "NextItem", "PreviousItem", "GoLive", "Show"].includes(typeof r === "string" ? r : Object.keys(r)[0])) state.overlay = "None";
      switch (typeof r === "string" ? r : Object.keys(r)[0]) {
        case "Next": move(1); state.live = state.preview; break;
        case "Previous": move(-1); state.live = state.preview; break;
        case "NextItem": move(1, true); break;
        case "PreviousItem": move(-1, true); break;
        case "GoLive": state.live = state.preview; break;
        case "Preview": state.preview = r.Preview; break;
        case "Show": state.live = r.Show; state.preview = r.Show; break;
        case "ToggleBlack": state.overlay = state.overlay === "Black" ? "None" : "Black"; break;
        case "ToggleIdle": state.overlay = state.overlay === "Idle" ? "None" : "Idle"; break;
        case "ClearAll": state.live = null; state.overlay = "None"; break;
      }
      pushLive();
      return ok;
    case "Outputs":
      switch (key) {
        case "List": return { type: "Outputs", data: state.outputs };
        case "ListDisplays": return { type: "Displays", data: [{ name: "Built-in Retina Display", width: 1728, height: 1117, x: 0, y: 0 }, { name: "Sanctuary projector", width: 1920, height: 1080, x: 1728, y: 0 }] };
        case "Put": { const i = state.outputs.findIndex((o: any) => o.id === arg.id); if (i >= 0) state.outputs[i] = arg; else state.outputs.push(arg); return ok; }
        case "Remove": state.outputs = state.outputs.filter((o: any) => o.id !== arg); return ok;
        case "BrowserSourceStatus": return { type: "BrowserSources", data: [] };
        case "BrowserSourceUrl": return { type: "Text", data: "http://127.0.0.1:7340/source/lyrics?key=…" };
        default: return ok;
      }
    case "Settings":
      if (key === "Put") settings = arg;
      if (key === "SaveBrand") {
        const id = settings.brand_theme ?? "brand-theme";
        const t = { ...JSON.parse(JSON.stringify(themes[0])), id, name: arg.name || "Our church", background: { Color: arg.primary }, text: { ...themes[0].text, color: arg.text ?? { r: 255, g: 255, b: 255, a: 255 }, font_family: arg.font_family || "Noto Sans" } };
        const i = themes.findIndex((x) => x.id === id); if (i >= 0) themes[i] = t; else themes.push(t);
        settings = { ...settings, brand: arg, brand_theme: id, default_theme: id };
      }
      if (key === "SetLogoBug") {
        if (!settings.brand) return { type: "Error", data: { message: "Add your church's logo in Setup first." } };
        settings = { ...settings, brand: { ...settings.brand, bug: arg } };
      }
      if (key === "Church" || key === "ResolveChurch") return { type: "Church", data: { shared: false, conflict: null, waiting: [] } };
      return { type: "Settings", data: settings };
    case "Update": return { type: "Update", data: { current: "0.1.0", state: "UpToDate", blocked: null } };
    case "Team": return { type: "Team", data: team() };
    case "Obs": return { type: "Obs", data: state.obs ?? obsOff() };
    case "About": return ok;
    case "Sound": return { type: "Sound", data: soundOn ? soundView() : { here: true, holder: "Demo laptop", away: false, nobody: false, output_ok: true, playing: null, problem: null, problem_item: null } };
  }
  return { type: "Error", data: { message: `mock: unhandled ${area}.${key}` } };
}
// This device on its own: presenting, holding every role.
const role = (name: string) => ({ role: name, holds: true, standing: "Holding", holder: "aa", holder_name: "Demo laptop", offline_secs: null, wait_secs: null, blocked: null, request: null, asking: null, replaced_by: null, always: [], offers_always: false, holder_waits_secs: null });
const team = () => ({
  name: "Demo laptop", fingerprint: "c339 03eb", addresses: ["192.168.1.20:7340"], pairing: null, paired: state.paired ?? [], nearby: [],
  presenter: { role: "Presenting", standing: "Presenting", holder: "aa", holder_name: "Demo laptop", epoch: 1, offline_secs: null, wait_secs: null, takeover_secs: 30, requests: [], asking: null, replaced_by: null,
    drive: { driving: null, active: null, requests: [], asking: null, can_drive: false, always: [], taken_back_by: null } },
  roles: ["Projection", "Operator", "Stream", "Audio"].map(role),
  scene_cue_waiting: false,
  sync: { state: "DeviceOnly", paired: (state.paired ?? []).length, connected: 0, reason: null, last_in_ms: null, last_out_ms: null },
});
const obsOff = () => ({ host: "127.0.0.1", port: 4455, has_password: false, connection: { Disconnected: { reason: "not running" } }, problem: null, relevant: true, stream: null, record: null, scenes: [], program_scene: null, held: false, scene_problem: null, audio: [], remote: false, addresses: [], pinned_address: null });
export const obsOn = () => ({ ...obsOff(), connection: { Connected: { obs_version: "30.2", websocket_version: "5.5" } }, stream: { active: true, reconnecting: false, timecode: "00:12:41", skipped_frames: 0, total_frames: 45678 }, record: null, scenes: ["Wide", "Speaker", "Lyrics"], program_scene: "Lyrics", audio: [{ name: "Mic", muted: false }] });
// Pretends a device was paired: the "connect another device" prompt goes away.
(window as any).__pairDevice = () => {
  state.paired = [{ device: "bb", fingerprint: "a1b2 c3d4", name: "Office PC", address: "192.168.1.30:7340", connected: true, problem: null, presenting: false }];
  emit({ type: "SyncStatus", data: { sync: { state: "Synced", paired: 1, connected: 1, reason: null, last_in_ms: null, last_out_ms: null } } });
};
(window as any).__soundOn = () => { soundOn = true; sunday.items.i3.backing = BACKING; };
(window as any).__obsOn = () => { state.obs = obsOn(); emit({ type: "Obs", data: state.obs }); };

export async function invoke(cmd: string, args: any) {
  if (cmd !== "api") throw new Error("mock: " + cmd);
  const res = api(args.request);
  if (res.type === "Error") console.warn(res.data.message, args.request);
  return res;
}
