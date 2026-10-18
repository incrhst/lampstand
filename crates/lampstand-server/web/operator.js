// Operator page: service order, preview and live, controls, library imports.
// Talks to the server over /ws (state + commands) and /api (service order, imports).

const $ = (id) => document.getElementById(id);
const el = (tag, props = {}, ...children) => {
  const e = Object.assign(document.createElement(tag), props);
  for (const c of children) e.append(c);
  return e;
};

let snap = null;      // latest Snapshot from the server
let service = null;   // the editable service order (GET/PUT /api/service)
let socket = null;

// --- Connection ------------------------------------------------------------------------

function connect() {
  socket = new WebSocket(`${location.protocol === "https:" ? "wss" : "ws"}://${location.host}/ws`);
  socket.onopen = () => {
    socket.send(JSON.stringify({ type: "hello", role: "operator", name: "Operator" }));
    setConn(true);
  };
  socket.onmessage = (ev) => {
    const msg = JSON.parse(ev.data);
    if (msg.type === "state") {
      if (snap && msg.revision < snap.revision) return;
      snap = msg;
      render();
    } else if (msg.type === "error") {
      console.warn(msg.message);
    }
  };
  socket.onclose = () => {
    setConn(false);
    setTimeout(connect, 1000);
  };
}

function setConn(ok) {
  const c = $("conn");
  c.textContent = ok ? "Connected" : "Reconnecting…";
  c.className = `conn ${ok ? "ok" : "bad"}`;
}

function command(cmd) {
  if (socket?.readyState === WebSocket.OPEN) socket.send(JSON.stringify({ type: "command", command: cmd }));
}

// --- Rendering ---------------------------------------------------------------------------

const KIND = { song: "Song", scripture: "Scripture", deck: "Deck", logo: "Logo" };

function render() {
  if (document.activeElement !== $("service-name")) $("service-name").value = snap.service_name;
  renderItems();
  renderScreen($("preview"), snap.preview_cue, "normal");
  renderScreen($("live"), snap.live_cue, snap.mode);
  $("preview-label").textContent = label(snap.preview, snap.preview_cue);
  $("live-label").textContent = label(snap.live, snap.live_cue);
  const badge = $("mode-badge");
  badge.hidden = snap.mode === "normal";
  badge.textContent = snap.mode;
  for (const m of ["clear", "black", "logo"]) $(`m-${m}`).classList.toggle("on", snap.mode === m);
  const notes = snap.live_cue?.notes ?? snap.preview_cue?.notes ?? "";
  $("notes").textContent = notes || "No notes for this slide.";
}

function label(pos, cue) {
  if (!pos || !cue) return "";
  const item = snap.items[pos.item];
  return `${item?.title ?? ""} · ${cue.label}`;
}

function renderItems() {
  const ol = $("items");
  const focusedCue = document.activeElement?.dataset?.cue;
  ol.replaceChildren();
  snap.items.forEach((item, i) => {
    const head = el("div", { className: "item-head" },
      el("span", { className: "kind", textContent: KIND[item.kind] ?? item.kind }),
      el("span", { className: "title", textContent: item.title, title: item.title }),
      el("button", { type: "button", textContent: "↑", title: "Move up", disabled: i === 0, onclick: () => moveItem(i, -1) }),
      el("button", { type: "button", textContent: "↓", title: "Move down", disabled: i === snap.items.length - 1, onclick: () => moveItem(i, 1) }),
      el("button", { type: "button", textContent: "✕", title: "Remove", onclick: () => removeItem(i) }),
    );
    const li = el("li", { className: "item" }, head);
    if (snap.live?.item === i) li.classList.add("is-live");
    if (item.problem) li.append(el("div", { className: "problem", textContent: item.problem }));
    const cues = el("div", { className: "cues" });
    item.cues.forEach((c, j) => {
      const b = el("button", { type: "button", className: "cue", textContent: c });
      b.dataset.cue = `${i}:${j}`;
      if (snap.preview?.item === i && snap.preview?.cue === j) b.classList.add("preview");
      if (snap.live?.item === i && snap.live?.cue === j) b.classList.add("live");
      // Click selects into preview; double-click goes straight to live.
      b.onclick = () => command({ kind: "select", pos: { item: i, cue: j } });
      b.ondblclick = () => command({ kind: "go_to", pos: { item: i, cue: j } });
      cues.append(b);
    });
    li.append(cues);
    ol.append(li);
  });
  if (focusedCue) ol.querySelector(`[data-cue="${focusedCue}"]`)?.focus();
}

function renderScreen(box, cue, mode) {
  box.replaceChildren();
  box.classList.toggle("dim", mode === "clear");
  if (mode === "black") return;
  if (mode === "logo") {
    if (snap.logo) box.append(el("img", { src: snap.logo, alt: "Logo" }));
    return;
  }
  if (!cue) return;
  if (cue.kind === "image") {
    box.append(el("img", { src: `${cue.url}?w=640`, alt: cue.label }));
  } else {
    box.append(el("div", { className: "text", innerText: cue.lines.join("\n") }));
    if (cue.footer) box.append(el("div", { className: "footer", textContent: cue.footer }));
  }
}

// --- Service order -----------------------------------------------------------------------

async function loadService() {
  service = await (await fetch("/api/service")).json();
}

async function saveService() {
  await fetch("/api/service", { method: "PUT", headers: { "content-type": "application/json" }, body: JSON.stringify(service) });
}

const newId = () => crypto.randomUUID?.() ?? `${Date.now()}-${Math.random().toString(16).slice(2)}`;

async function addItem(source) {
  await loadService();
  service.items.push({ id: newId(), ...source });
  await saveService();
}

async function moveItem(i, d) {
  await loadService();
  const [it] = service.items.splice(i, 1);
  service.items.splice(i + d, 0, it);
  await saveService();
}

async function removeItem(i) {
  await loadService();
  service.items.splice(i, 1);
  await saveService();
}

$("service-name").addEventListener("change", async (e) => {
  await loadService();
  service.name = e.target.value;
  await saveService();
});

// --- Adding items ------------------------------------------------------------------------

let songTimer = 0;
let songHits = [];
let songSel = 0;

$("song-q").addEventListener("input", () => {
  clearTimeout(songTimer);
  songTimer = setTimeout(searchSongs, 120);
});
$("song-q").addEventListener("focus", searchSongs);
$("song-q").addEventListener("blur", () => setTimeout(() => ($("song-results").hidden = true), 150));
$("song-q").addEventListener("keydown", (e) => {
  if (e.key === "ArrowDown") { songSel = Math.min(songSel + 1, songHits.length - 1); drawHits(); e.preventDefault(); }
  if (e.key === "ArrowUp") { songSel = Math.max(songSel - 1, 0); drawHits(); e.preventDefault(); }
  if (e.key === "Enter" && songHits[songSel]) { pickSong(songHits[songSel]); e.preventDefault(); }
  if (e.key === "Escape") e.target.blur();
});

async function searchSongs() {
  const q = $("song-q").value;
  songHits = await (await fetch(`/api/library/songs?q=${encodeURIComponent(q)}`)).json();
  songSel = 0;
  drawHits();
}

function drawHits() {
  const ul = $("song-results");
  ul.replaceChildren();
  if (!songHits.length) {
    ul.append(el("li", { textContent: $("song-q").value ? "No songs match." : "No songs yet: import some from Library." }));
  }
  songHits.forEach((s, i) => {
    const li = el("li", {}, s.title, el("small", { textContent: [s.authors.join(", "), s.first_line].filter(Boolean).join(" · ") }));
    li.setAttribute("aria-selected", String(i === songSel));
    li.onmousedown = (e) => { e.preventDefault(); pickSong(s); };
    ul.append(li);
  });
  ul.hidden = false;
}

async function pickSong(s) {
  $("song-q").value = "";
  $("song-results").hidden = true;
  await addItem({ kind: "song", song: s.id });
}

$("scripture-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  const ref = $("scripture-ref").value.trim();
  const tr = $("scripture-tr").value;
  const msg = $("scripture-msg");
  if (!ref) return;
  if (!tr) { msg.hidden = false; msg.textContent = "Import a Bible first (Library)."; return; }
  const r = await fetch(`/api/scripture?ref=${encodeURIComponent(ref)}&translation=${encodeURIComponent(tr)}`);
  const body = await r.json();
  if (!r.ok) { msg.hidden = false; msg.textContent = body.error; return; }
  msg.hidden = true;
  $("scripture-ref").value = "";
  await addItem({ kind: "scripture", reference: body.reference, translation: body.translation });
});

$("deck-pick").addEventListener("change", async (e) => {
  if (!e.target.value) return;
  await addItem({ kind: "deck", deck: e.target.value });
  e.target.value = "";
});

$("deck-file").addEventListener("change", async (e) => {
  const res = await upload("/api/import/deck", e.target.files);
  e.target.value = "";
  if (Array.isArray(res)) for (const d of res) await addItem({ kind: "deck", deck: d.id });
  loadLibrary();
});

$("add-logo").addEventListener("click", () => addItem({ kind: "logo" }));

// --- Library -----------------------------------------------------------------------------

async function loadLibrary() {
  const lib = await (await fetch("/api/library")).json();
  const tr = $("scripture-tr");
  const keep = tr.value;
  tr.replaceChildren(...lib.bibles.map((b) => el("option", { value: b.abbreviation, textContent: b.abbreviation, title: b.name })));
  if (keep) tr.value = keep;
  const dp = $("deck-pick");
  dp.replaceChildren(el("option", { value: "", textContent: "Add a deck…" }),
    ...lib.decks.map((d) => el("option", { value: d.id, textContent: d.problem ? `${d.name} (can't open)` : `${d.name} (${d.slides} slides)` })));
  $("lib-summary").textContent =
    `${lib.songs} songs · ${lib.bibles.length} Bible translations · ${lib.decks.length} decks · ${lib.logo ? "logo set" : "no logo"}. Stored in ${lib.data_dir}`;
}

async function upload(url, fileList, extra = {}) {
  const fd = new FormData();
  for (const f of fileList) fd.append("file", f, f.name);
  for (const [k, v] of Object.entries(extra)) fd.append(k, v);
  const r = await fetch(url, { method: "POST", body: fd });
  const text = await r.text();
  let body = null;
  try { body = JSON.parse(text); } catch { body = text; }
  const rep = $("lib-report");
  if (!r.ok) { rep.hidden = false; rep.textContent = body?.error ?? text; return null; }
  if (body && (body.added || body.notes)) {
    rep.hidden = false;
    rep.textContent = [`Added: ${body.added.join(", ") || "nothing"}`, ...body.notes].join("\n");
  }
  return body;
}

$("open-library").addEventListener("click", () => { loadLibrary(); $("library").showModal(); });
$("songs-file").addEventListener("change", async (e) => { await upload("/api/import/songs", e.target.files); e.target.value = ""; loadLibrary(); });
$("bible-file").addEventListener("change", async (e) => {
  await upload("/api/import/bible", e.target.files, { abbreviation: $("bible-abbr").value });
  e.target.value = "";
  loadLibrary();
});
$("logo-file").addEventListener("change", async (e) => { await upload("/api/logo", e.target.files); e.target.value = ""; loadLibrary(); });

// --- Controls and keys -------------------------------------------------------------------

$("go").onclick = () => command({ kind: "go_live" });
$("next").onclick = () => command({ kind: "next" });
$("prev").onclick = () => command({ kind: "prev" });
for (const m of ["clear", "black", "logo"]) $(`m-${m}`).onclick = () => command({ kind: "mode", mode: m });

const KEYS = {
  ArrowRight: { kind: "next" }, ArrowDown: { kind: "next" }, PageDown: { kind: "next" }, " ": { kind: "next" },
  ArrowLeft: { kind: "prev" }, ArrowUp: { kind: "prev" }, PageUp: { kind: "prev" },
  Enter: { kind: "go_live" },
  b: { kind: "mode", mode: "black" }, ".": { kind: "mode", mode: "black" },
  w: { kind: "mode", mode: "clear" }, ",": { kind: "mode", mode: "clear" },
  l: { kind: "mode", mode: "logo" },
};

document.addEventListener("keydown", (e) => {
  const t = e.target;
  if (t.closest?.("input, select, textarea, dialog") || e.metaKey || e.ctrlKey || e.altKey) return;
  // Enter and Space on a focused button press that button instead.
  if ((e.key === "Enter" || e.key === " ") && t.tagName === "BUTTON") return;
  const cmd = KEYS[e.key.length === 1 ? e.key.toLowerCase() : e.key];
  if (cmd) { command(cmd); e.preventDefault(); }
});

loadLibrary();
connect();
