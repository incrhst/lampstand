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
    { label: "Slide 2", lines: ["I know my sheep", "and my sheep know me"], steps: 2, notes: "Pause here. Ask: who knows your name?" },
    { label: "Slide 3", lines: ["He lays down his life"], steps: 1, notes: "Slow down." },
  ],
};
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
const state: any = { services: [welcome, sunday], outputs: [], loaded: null, loopOverrides: {} as Record<string, number | null>, live: null, preview: null, overlay: "None", obs: null, scene: (window as any).__scene ?? "default" };
if (state.scene !== "fresh") state.outputs.push({ id: "out1", name: "Sanctuary projector", role: { Audience: { primary: true } }, target: { Window: { width: 1920, height: 1080 } }, safe_area: { top: 0.03, right: 0.03, bottom: 0.03, left: 0.03 }, background: "Bleed", theme: null, enabled: true });
(window as any).__mock = state;

const ID = (n: string) => n;
const sorted = (s: any) => Object.entries(s.items).sort((a: any, b: any) => a[1].position.localeCompare(b[1].position));
function slidesFor(kind: any) {
  if (kind.Header) return [];
  if (kind.Song) { const s = songs.find((x) => x.id === kind.Song.song)!; return s.sections.map((sec) => ({ label: sec.label, lines: sec.lines, steps: 0, notes: "" })); }
  if (kind.Scripture) return [{ label: "John 3:16", lines: [verses["John 3:16"][0][2]], steps: 0, notes: "" }];
  if (kind.Deck) return deckSlides[kind.Deck.deck] ?? [];
  return [];
}
function titleFor(kind: any) {
  if (kind.Header) return kind.Header.title;
  if (kind.Song) return songs.find((x) => x.id === kind.Song.song)!.title;
  if (kind.Scripture) return "John 3:16";
  if (kind.Deck) return decks.find((d) => d.id === kind.Deck.deck)!.title;
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
function pushLive() { emit({ type: "Live", data: { live: state.live, preview: state.preview, idle: false, overlay: state.overlay } }); }
const flat = () => (state.loaded ? state.loaded.items.flatMap((i: any) => i.slides.map((_: any, n: number) => ({ item: i.id, slide: n, step: 0 }))) : []);
function move(dir: number, byItem = false) {
  const f = flat(); if (!f.length) return;
  const base = state.live ?? state.preview; let at = base ? f.findIndex((c: any) => c.item === base.item && c.slide === base.slide) : -1;
  if (byItem) { const cur = base?.item; let n = at; while (n >= 0 && n < f.length && f[n].item === cur) n += dir; at = n < 0 ? 0 : Math.min(n, f.length - 1); if (dir < 0) { const it = f[at].item; at = f.findIndex((c: any) => c.item === it); } }
  else at = Math.max(0, Math.min(f.length - 1, at + dir));
  state.preview = f[Math.max(0, at)];
}
const svgThumb = (label: string) => "data:image/svg+xml;utf8," + encodeURIComponent(`<svg xmlns='http://www.w3.org/2000/svg' width='320' height='180'><rect width='320' height='180' fill='#0b0e16'/><text x='160' y='95' fill='#f5f1e8' font-family='serif' font-size='20' text-anchor='middle'>${label}</text></svg>`);

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
        case "ListThemes": return { type: "Themes", data: [] };
        case "ListTranslations": return { type: "Translations", data: [{ id: "kjv", name: "King James Version", abbreviation: "KJV", language: "en", licence: "PublicDomain", text: "kjv" }] };
        case "LookupPassage": { const ref = Object.keys(verses).find((k) => arg.reference.toLowerCase().startsWith(k.toLowerCase().split(":")[0].toLowerCase())) ?? "John 3:16"; return { type: "Passage", data: { passage: { book: 43, start: { chapter: 3, verse: 16 }, end: { chapter: 3, verse: 16 } }, display: ref, verses: verses[ref] } }; }
        case "ListSongDatabases": return { type: "SongDatabases", data: [{ code: "en", name: "English", source: "Free song database" }] };
        case "ListDecks": return { type: "Decks", data: state.scene === "empty" ? [] : decks };
        case "ListMedia": return { type: "MediaFiles", data: [] };
        case "DeckThumbnail": case "MediaThumbnail": return { type: "Thumbnail", data: svgThumb("Slides") };
      }
      break;
    case "Service":
      switch (key) {
        case "List": return { type: "Services", data: state.services.map((s: any) => ({ id: s.id, title: s.title, starts_at: s.starts_at, time_zone: s.time_zone, items: Object.values(s.items).filter((i: any) => !i.kind.Header).length })) };
        case "Get": return { type: "Service", data: state.services.find((s: any) => s.id === arg) };
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
        case "SetLoop": { const s = state.services.find((x: any) => x.id === arg.service); s.items[arg.item].loop_seconds = arg.seconds; return { type: "Service", data: s }; }
        case "SetMediaLooped": { const s = state.services.find((x: any) => x.id === arg.service); s.items[arg.item].kind.Media.looped = arg.looped; return { type: "Service", data: s }; }
        case "SetObsScene": case "Reschedule": return ok;
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
        case "SlideThumbnail": return { type: "Thumbnail", data: svgThumb("Slide") };
        case "Readiness": return { type: "Text", data: "ready" };
      }
      break;
    case "Live":
      switch (typeof r === "string" ? r : Object.keys(r)[0]) {
        case "Next": move(1); state.live = state.preview; break;
        case "Previous": move(-1); state.live = state.preview; break;
        case "NextItem": move(1, true); break;
        case "PreviousItem": move(-1, true); break;
        case "GoLive": state.live = state.preview; break;
        case "Preview": state.preview = r.Preview; break;
        case "Show": state.live = r.Show; state.preview = r.Show; break;
        case "ToggleBlack": state.overlay = state.overlay === "Black" ? "None" : "Black"; break;
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
    case "Settings": return { type: "Settings", data: { clicker_mode: false } };
    case "Update": return { type: "Update", data: { current: "0.1.0", state: "UpToDate", blocked: null } };
    case "Team": return { type: "Team", data: { name: "Demo laptop", fingerprint: "c339 03eb", addresses: ["192.168.1.20:7340"], pairing: null, paired: [], nearby: [] } };
    case "Obs": return { type: "Obs", data: state.obs ?? obsOff() };
    case "About": return ok;
  }
  return { type: "Error", data: { message: `mock: unhandled ${area}.${key}` } };
}
const obsOff = () => ({ host: "127.0.0.1", port: 4455, has_password: false, connection: { Disconnected: { reason: "not running" } }, problem: null, relevant: true, stream: null, record: null, scenes: [], program_scene: null, held: false, scene_problem: null, audio: [] });
export const obsOn = () => ({ ...obsOff(), connection: { Connected: { obs_version: "30.2", websocket_version: "5.5" } }, stream: { active: true, reconnecting: false, timecode: "00:12:41", skipped_frames: 0, total_frames: 45678 }, record: null, scenes: ["Wide", "Speaker", "Lyrics"], program_scene: "Lyrics", audio: [{ name: "Mic", muted: false }] });
(window as any).__obsOn = () => { state.obs = obsOn(); emit({ type: "Obs", data: state.obs }); };

export async function invoke(cmd: string, args: any) {
  if (cmd !== "api") throw new Error("mock: " + cmd);
  const res = api(args.request);
  if (res.type === "Error") console.warn(res.data.message, args.request);
  return res;
}
