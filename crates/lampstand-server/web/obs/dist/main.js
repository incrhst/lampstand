// The OBS browser source: connects to /obs/<output>/ws and draws what
// arrives. Transparent where nothing is painted, so OBS keys it over video.
import { build, preload } from "./render.js";
const params = new URLSearchParams(location.search);
const key = params.get("k") ?? "";
const output = location.pathname.split("/").filter(Boolean)[1] ?? "";
const q = `k=${encodeURIComponent(key)}`;
const resources = {
    blobUrl: (hash) => `/blobs/${hash}?o=${encodeURIComponent(output)}&${q}`,
    images: new Map(),
};
const stage = document.getElementById("stage");
let size = { width: 1920, height: 1080 };
/** The scene is laid out in its own units and scaled to the source's size. */
function fitStage() {
    stage.style.width = `${size.width}px`;
    stage.style.height = `${size.height}px`;
    stage.style.transform = `scale(${innerWidth / size.width},${innerHeight / size.height})`;
}
async function show(msg) {
    if ("scene" in msg) {
        await preload(msg.scene, resources);
        size = { width: msg.scene.width, height: msg.scene.height };
        const content = build(msg.scene, resources);
        fitStage();
        stage.replaceChildren(content);
    }
    else {
        const img = new Image();
        img.src = `/frames/${encodeURIComponent(output)}/${msg.frame}.webp?${q}`;
        img.style.cssText = `position:absolute;left:0;top:0;width:${msg.width}px;height:${msg.height}px;`;
        await img.decode();
        size = { width: msg.width, height: msg.height };
        fitStage();
        stage.replaceChildren(img);
    }
    window.lampstandSeq = msg.seq;
    document.documentElement.dataset.seq = String(msg.seq);
}
// Messages are drawn in order, but only the newest of a burst.
let pending = null;
let drawing = false;
async function enqueue(msg) {
    pending = msg;
    if (drawing)
        return;
    drawing = true;
    while (pending) {
        const next = pending;
        pending = null;
        try {
            await show(next);
        }
        catch (e) {
            console.error("lampstand: cannot draw the scene", e);
        }
    }
    drawing = false;
}
function connect(delay = 500) {
    const scheme = location.protocol === "https:" ? "wss:" : "ws:";
    const ws = new WebSocket(`${scheme}//${location.host}/obs/${encodeURIComponent(output)}/ws?${q}`);
    ws.onmessage = (e) => {
        try {
            void enqueue(JSON.parse(String(e.data)));
        }
        catch (err) {
            console.error("lampstand: bad message", err);
        }
    };
    ws.onopen = () => (delay = 500);
    // The last scene stays up while we reconnect.
    ws.onclose = () => setTimeout(() => connect(Math.min(delay * 2, 5000)), delay);
}
addEventListener("resize", fitStage);
fitStage();
connect();
