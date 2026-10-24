// node --test site/
const { test } = require("node:test");
const assert = require("node:assert/strict");
const { platformOf, installers, best, megabytes, versionOf, LINUX_LAST } = require("./download.js");

const release = {
  tag_name: "v0.1.0",
  assets: [
    { name: "Lampstand_0.1.0_universal.dmg", browser_download_url: "https://x/mac.dmg", size: 109051904 },
    { name: "Lampstand.app.tar.gz", browser_download_url: "https://x/app.tar.gz", size: 5 },
    { name: "Lampstand.app.tar.gz.sig", browser_download_url: "https://x/app.sig", size: 1 },
    { name: "Lampstand_0.1.0_x64-setup.exe", browser_download_url: "https://x/win.exe", size: 17825792 },
    { name: "Lampstand_0.1.0_x64-setup.exe.sig", browser_download_url: "https://x/win.sig", size: 1 },
    { name: "Lampstand_0.1.0_amd64.deb", browser_download_url: "https://x/l.deb", size: 9000000 },
    { name: "Lampstand_0.1.0_amd64.AppImage", browser_download_url: "https://x/l.AppImage", size: 80000000 },
    { name: "latest.json", browser_download_url: "https://x/latest.json", size: 1 },
  ],
};

test("it knows the visitor's computer, and not a phone's", () => {
  assert.equal(platformOf("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)", "MacIntel"), "mac");
  assert.equal(platformOf("Mozilla/5.0 (Windows NT 10.0; Win64; x64)", "Win32"), "windows");
  assert.equal(platformOf("Mozilla/5.0 (X11; Linux x86_64)", "Linux x86_64"), "linux");
  assert.equal(platformOf("Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X)", "iPhone"), null);
  assert.equal(platformOf("Mozilla/5.0 (Linux; Android 14)", "Linux armv8l"), null);
  assert.equal(platformOf("Mozilla/5.0 (Macintosh; Intel Mac OS X) Mobile", "MacIntel"), null);
  assert.equal(platformOf("", ""), null);
});

test("it finds one installer per kind and ignores signatures, updater files and the rest", () => {
  const found = installers(release);
  assert.deepEqual(found.map((i) => [i.platform, i.name]), [
    ["mac", "Lampstand_0.1.0_universal.dmg"],
    ["windows", "Lampstand_0.1.0_x64-setup.exe"],
  ]);
});

test("Linux gets no installer, even from an older release that has them, and points to 0.7.0", () => {
  assert.equal(best(installers(release), "linux"), null);
  assert.equal(LINUX_LAST.version, "0.7.0");
  assert.equal(LINUX_LAST.url, "https://github.com/incrhst/lampstand/releases/tag/v0.7.0");
  assert.equal(best(installers(release), "mac").url, "https://x/mac.dmg");
  assert.equal(best(installers(release), null), null);
});

test("a release with nothing for the visitor's computer gives none", () => {
  const winOnly = { tag_name: "v1", assets: [release.assets[3]] };
  assert.equal(best(installers(winOnly), "mac"), null);
  assert.deepEqual(installers({ tag_name: "v1" }), []);
  assert.deepEqual(installers(null), []);
});

test("sizes and versions read plainly", () => {
  assert.equal(megabytes(109051904), "104 MB");
  assert.equal(megabytes(2048), "2 KB");
  assert.equal(versionOf({ tag_name: "v0.1.0" }), "0.1.0");
  assert.equal(versionOf({ tag_name: "0.2.0" }), "0.2.0");
  assert.equal(versionOf(null), "");
});
