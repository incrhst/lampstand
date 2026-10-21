// The Download button: picks the installer for the visitor's computer from the
// latest release on GitHub. Without JavaScript, or if GitHub cannot be reached,
// the button simply goes to the releases page.
(() => {
  const REPO = "incrhst/lampstand";
  const RELEASES = `https://github.com/${REPO}/releases`;

  /** "mac", "windows" or "linux" for the visitor's computer, or null for a phone or anything else. */
  function platformOf(ua, platform) {
    const text = `${platform || ""} ${ua || ""}`.toLowerCase();
    // Phones and tablets cannot run it, however Mac-like they pretend to be.
    if (/iphone|ipad|ipod|android|mobile/.test(text)) return null;
    if (/mac/.test(text)) return "mac";
    if (/win/.test(text)) return "windows";
    if (/linux|x11|cros/.test(text)) return "linux";
    return null;
  }

  /** What each platform's installer is called in a release, best first. */
  const KINDS = {
    mac: [{ test: (n) => n.endsWith(".dmg"), label: "Mac (Apple silicon and Intel)" }],
    windows: [{ test: (n) => n.endsWith("-setup.exe") || (n.endsWith(".exe") && !n.endsWith(".sig")), label: "Windows" }],
    linux: [
      { test: (n) => n.endsWith(".appimage"), label: "Linux (AppImage)" },
      { test: (n) => n.endsWith(".deb"), label: "Linux (.deb, Debian and Ubuntu)" },
    ],
  };

  /** The release's installer files, by platform: [{ platform, label, url, name, size }]. */
  function installers(release) {
    const assets = (release && release.assets) || [];
    const found = [];
    for (const [platform, kinds] of Object.entries(KINDS)) {
      for (const kind of kinds) {
        const asset = assets.find((a) => kind.test(String(a.name).toLowerCase()));
        if (asset) found.push({ platform, label: kind.label, url: asset.browser_download_url, name: asset.name, size: asset.size });
      }
    }
    return found;
  }

  /** The installer to offer first for `platform`, or the first one at all. */
  function best(list, platform) {
    return list.find((i) => i.platform === platform) || null;
  }

  function megabytes(bytes) {
    return bytes >= 1048576 ? `${Math.round(bytes / 1048576)} MB` : `${Math.max(1, Math.round(bytes / 1024))} KB`;
  }

  /** "v0.1.0" -> "0.1.0". */
  const versionOf = (release) => String((release && release.tag_name) || "").replace(/^v/, "");

  if (typeof module !== "undefined") module.exports = { platformOf, installers, best, megabytes, versionOf };
  if (typeof document === "undefined") return;

  const button = document.getElementById("download");
  const note = document.getElementById("download-note");
  const others = document.getElementById("download-others");
  if (!button || !note) return;

  const platform = platformOf(navigator.userAgent, (navigator.userAgentData && navigator.userAgentData.platform) || navigator.platform);

  fetch(`https://api.github.com/repos/${REPO}/releases/latest`, { headers: { Accept: "application/vnd.github+json" } })
    .then((r) => (r.ok ? r.json() : Promise.reject(new Error(String(r.status)))))
    .then((release) => {
      const list = installers(release);
      const version = versionOf(release);
      const mine = best(list, platform);
      if (mine) {
        button.href = mine.url;
        button.querySelector(".btn-label").textContent = `Download for ${mine.label.split(" (")[0]}`;
        note.textContent = `Version ${version} · ${megabytes(mine.size)} · free, GPL-3.0`;
      } else {
        button.href = release.html_url || RELEASES;
        button.querySelector(".btn-label").textContent = "Download Lampstand";
        note.textContent = `Version ${version}. Choose the installer for your computer on the next page.`;
      }
      if (others && list.length) {
        others.hidden = false;
        const ul = others.querySelector("ul");
        ul.replaceChildren(
          ...list.map((i) => {
            const li = document.createElement("li");
            const a = document.createElement("a");
            a.href = i.url;
            a.textContent = `${i.label} · ${megabytes(i.size)}`;
            li.append(a);
            return li;
          }),
        );
      }
    })
    .catch(() => {
      // No release yet, or GitHub unreachable: the plain link stays.
      button.href = RELEASES;
      note.textContent = "The first release is on its way. Star the project on GitHub to hear about it.";
    });
})();
