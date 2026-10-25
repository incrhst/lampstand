import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { fileURLToPath } from "node:url";

// The phone remote's page (T100): a second entry, built into the server
// crate and checked in there (as `web/obs/dist` is), so cargo needs no Node.
// Fixed file names, because the server embeds them by name; small assets
// (the keep-awake video) are inlined so there is nothing else to serve.
export default defineConfig({
  plugins: [svelte({ configFile: fileURLToPath(new URL("svelte.config.js", import.meta.url)) })],
  root: "src/remote",
  base: "/remote/assets/",
  clearScreen: false,
  build: {
    outDir: "../../../crates/lampstand-server/web/remote/dist",
    emptyOutDir: true,
    // Safari 16 and Chrome on Android (decision 0006).
    target: ["es2020", "safari16", "chrome100"],
    assetsInlineLimit: 16 * 1024,
    modulePreload: false,
    rolldownOptions: {
      output: {
        entryFileNames: "remote.js",
        assetFileNames: "remote[extname]",
        codeSplitting: false,
      },
    },
  },
});
