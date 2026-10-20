import { defineConfig } from "@playwright/test";

// The tests start the Rust fixture server themselves (see tests/obs.spec.ts),
// so there is no webServer here. Building it can take minutes on a cold cache.
export default defineConfig({
  testDir: "tests",
  timeout: 60_000,
  workers: 1,
  reporter: [["list"]],
  use: {
    viewport: { width: 1920, height: 1080 },
    deviceScaleFactor: 1,
    launchOptions: {
      // Text positions must not depend on the machine: no hinting, no LCD
      // antialiasing, sRGB output.
      args: ["--font-render-hinting=none", "--disable-lcd-text", "--force-color-profile=srgb"],
    },
  },
  projects: [{ name: "chromium", use: { browserName: "chromium" } }],
});
