// Runs the real Svelte UI in a plain browser with the Tauri bridge replaced
// by a scripted fake, so the video's screenshots can be retaken without the
// native app and without touching anyone's data folder.
import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { fileURLToPath } from "node:url";

const here = (p: string) => fileURLToPath(new URL(p, import.meta.url));

export default defineConfig({
  root: here("../../app"),
  plugins: [svelte()],
  clearScreen: false,
  resolve: {
    alias: [
      { find: "@tauri-apps/api/core", replacement: here("./mock-core.ts") },
      { find: "@tauri-apps/api/event", replacement: here("./mock-event.ts") },
      { find: "@tauri-apps/plugin-dialog", replacement: here("./mock-dialog.ts") },
    ],
  },
  server: { port: 1431, strictPort: true, fs: { allow: [here("../..")] } },
});
