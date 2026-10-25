// Module hooks for the component tests (`*.test.ts` that render a kit
// component): Node runs .svelte files compiled for the server, and finds the
// .ts modules they import without an extension, as Vite does.
// Registered with `register("./svelteHooks.mjs", import.meta.url)`.

import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { compile } from "svelte/compiler";

export async function resolve(specifier, context, next) {
  const parent = context.parentURL ?? "";
  const relative = specifier.startsWith("./") || specifier.startsWith("../");
  if (relative && !/\.[a-z]+$/i.test(specifier) && /\.(svelte|ts)$/.test(parent)) {
    try {
      return await next(`${specifier}.ts`, context);
    } catch {
      // Not a .ts module: resolve it as written.
    }
  }
  return next(specifier, context);
}

export async function load(url, context, next) {
  if (!url.endsWith(".svelte")) return next(url, context);
  const filename = fileURLToPath(url);
  const source = await readFile(filename, "utf8");
  const { js } = compile(source, { filename, generate: "server", css: "external" });
  return { format: "module", source: js.code, shortCircuit: true };
}
