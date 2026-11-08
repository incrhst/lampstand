// The role bar and the roster's holder line as components (kit/RoleBar.svelte,
// kit/RoleHolder.svelte), rendered to HTML on the server side of Svelte:
// segment order, the empty state, the label, and a record this device could
// not check (#184). Each component is compiled here, with its imports
// pointed at the files beside it, so no bundler is needed.
import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { compile } from "svelte/compiler";
import { render } from "svelte/server";
import type { RoleName, RoleView, TeamStatus } from "./api";

const out = mkdtempSync(join(tmpdir(), "lampstand-kit-"));
const compiled = new Map<string, string>();

/** Compiles a component (and the components it imports) to a module URL. */
function compileFile(file: URL): string {
  const done = compiled.get(file.href);
  if (done) return done;
  const { js } = compile(readFileSync(file, "utf8"), { generate: "server", filename: fileURLToPath(file) });
  const code = js.code.replace(/from\s+["']([^"']+)["']/g, (_, spec: string) => {
    if (!spec.startsWith(".")) return `from "${import.meta.resolve(spec)}"`;
    const target = new URL(spec, file);
    if (spec.endsWith(".svelte")) return `from "${compileFile(target)}"`;
    const ts = new URL(`${target.href}.ts`);
    return `from "${existsSync(fileURLToPath(ts)) ? ts.href : target.href}"`;
  });
  const path = join(out, `${basename(fileURLToPath(file))}.js`);
  writeFileSync(path, code);
  const url = pathToFileURL(path).href;
  compiled.set(file.href, url);
  return url;
}

async function component(name: string) {
  return (await import(compileFile(new URL(`./kit/${name}`, import.meta.url)))).default;
}

const RoleBar = await component("RoleBar.svelte");
const RoleHolder = await component("RoleHolder.svelte");

const bar = (held: RoleName[], unchecked: RoleName[] = []) => render(RoleBar, { props: { held, unchecked } }).body;
const order = (html: string) => [...html.matchAll(/data-role="(\w+)"/g)].map((m) => m[1]);
const label = (html: string) => html.match(/aria-label="([^"]*)"/)?.[1];

test("a segment per role held, always in the order Projection, Operator, Stream, Audio", () => {
  const html = bar(["Audio", "Stream", "Projection"]);
  assert.deepEqual(order(html), ["Projection", "Stream", "Audio"]);
  assert.equal(label(html), "Runs the screens, the stream and the sound");
  assert.match(html, /title="Runs the screens, the stream and the sound"/);
  assert.match(html, /role="img"/);
  assert.deepEqual(order(bar(["Operator", "Projection", "Audio", "Stream"])), ["Projection", "Operator", "Stream", "Audio"]);
});

test("a device that runs nothing shows an empty bar labelled Planning", () => {
  const html = bar([]);
  assert.deepEqual(order(html), []);
  assert.match(html, /class="bar[^"]*\bempty\b/);
  assert.equal(label(html), "Planning");
});

test("a role whose record could not be checked is hatched and named in the label", () => {
  const html = bar(["Projection", "Stream"], ["Stream", "Audio"]);
  assert.deepEqual(order(html), ["Projection", "Stream"]);
  assert.match(html, /class="segment stream[^"]*\bunchecked\b[^"]*" data-role="Stream"/);
  assert.doesNotMatch(html, /class="segment projection[^"]*\bunchecked\b/);
  assert.equal(label(html), "Runs the screens and the stream. Can't check who runs the stream");
});

const view = (role: RoleName, over: Partial<RoleView> = {}): RoleView => ({
  role,
  holds: false,
  standing: "Ask",
  holder: "aa",
  holder_name: "Mary's laptop",
  offline_secs: null,
  wait_secs: null,
  blocked: null,
  request: null,
  asking: null,
  replaced_by: null,
  always: [],
  holder_waits_secs: null,
  offers_always: role === "Operator" || role === "Stream",
  refused_by: null,
  cooldown_secs: null,
  unverified: false,
  ...over,
});
const church = (roles: RoleView[]): TeamStatus =>
  ({ roles, paired: [{ device: "aa", name: "Mary's laptop" }], presenter: {}, scene_cue_waiting: false }) as unknown as TeamStatus;
const holder = (team: TeamStatus, role: RoleName) => render(RoleHolder, { props: { team, role } }).body;

test("the roster says plainly when a role's record could not be checked, and what to do", () => {
  const team = church([
    view("Projection", { holds: true, standing: "Holding", holder: "me", holder_name: "Sanctuary" }),
    view("Stream", { unverified: true }),
    view("Audio"),
  ]);
  const html = holder(team, "Stream");
  assert.match(html, /Mary's laptop/);
  assert.match(html, /role="note"/);
  assert.match(html, /Can't check who runs the stream\./);
  assert.match(html, /unpair one of the devices below and pair it again/);
  // The holder's bar has the stream and the sound, the stream hatched.
  assert.deepEqual(order(html), ["Stream", "Audio"]);
  assert.match(html, /data-role="Stream"/);
  assert.match(html, /class="segment stream[^"]*\bunchecked\b/);
  assert.doesNotMatch(html, /class="segment audio[^"]*\bunchecked\b/);
  // A role this device could check has no note (the holder's bar still
  // marks its stream).
  const fine = holder(team, "Audio");
  assert.doesNotMatch(fine, /role="note"/);
  assert.doesNotMatch(fine, /unpair/);
  // This device's own row: its own bar.
  assert.deepEqual(order(holder(team, "Projection")), ["Projection"]);
  assert.match(holder(team, "Projection"), /This device/);
});

test("nobody holds a role: no bar, just Nobody", () => {
  const html = holder(church([view("Stream", { holder: null, holder_name: "", standing: "Open" })]), "Stream");
  assert.doesNotMatch(html, /class="bar/);
  assert.match(html, /Nobody/);
});
