<!-- Setup › Church look: the quick start. A church says its name, adds its
     logo, picks two or three colours and a font, and Lampstand makes a
     matching theme and uses it everywhere that doesn't choose its own. The
     logo, small and centred on its colour, is also the default idle screen,
     which the Idle button in Present puts on the screens, and the corner
     logo bug. -->
<script lang="ts">
  import { open as pickFiles } from "@tauri-apps/plugin-dialog";
  import { request, type Brand, type BugCorner, type BugSize } from "../../lib/api";
  import { imageExtensions, onePath } from "../../lib/media";
  import { loadIdlePicture } from "../../lib/idleThumb";
  import { live, refreshSettings, refreshThemes, report } from "../../lib/state.svelte";
  import { STARTER_THEME_ID, bundledFonts, fromHex, readableOn, themeFromBrand, toHex } from "../../lib/themes";
  import Banner from "../../lib/kit/Banner.svelte";
  import Button from "../../lib/kit/Button.svelte";
  import Panel from "../../lib/kit/Panel.svelte";
  import ThemeSketch from "../../lib/kit/ThemeSketch.svelte";
  import SlidePreview from "../../lib/kit/SlidePreview.svelte";
  import Segmented from "../../lib/kit/Segmented.svelte";
  import Switch from "../../lib/kit/Switch.svelte";
  import BrandFile from "../../lib/kit/BrandFile.svelte";
  import ChurchShare from "../../lib/kit/ChurchShare.svelte";

  let { onthemes }: { onthemes?: () => void } = $props();

  const start: Brand = {
    name: "",
    logo: null,
    primary: { r: 27, g: 42, b: 92, a: 255 },
    text: null,
    accent: { r: 240, g: 190, b: 60, a: 255 },
    font_family: "Noto Sans",
    bug: { enabled: false, corner: "LowerRight", size: "Small", opacity: 80 },
  };

  const corners: { id: BugCorner; label: string }[] = [
    { id: "LowerLeft", label: "Lower left" },
    { id: "LowerRight", label: "Lower right" },
  ];
  const sizes: { id: BugSize; label: string }[] = [
    { id: "Small", label: "Small" },
    { id: "Medium", label: "Medium" },
    { id: "Large", label: "Large" },
  ];

  let brand: Brand = $state(JSON.parse(JSON.stringify(live.settings?.brand ?? start)));
  let customText = $state(!!live.settings?.brand?.text);
  let logoPicture: string | null = $state(null);
  let busy = $state(false);
  let done = $state(false);

  refreshSettings().then(() => {
    // The saved brand arrives after the page opens on a first visit.
    if (live.settings?.brand && !brand.name && !done) {
      brand = JSON.parse(JSON.stringify(live.settings.brand));
      customText = !!brand.text;
    }
  });
  refreshThemes();

  let starter = $derived(live.themes.find((t) => t.id === STARTER_THEME_ID) ?? live.themes[0]);
  let theme = $derived(starter ? themeFromBrand(brand, starter) : null);
  let textColor = $derived(brand.text ?? readableOn(brand.primary));

  $effect(() => {
    const blob = brand.logo;
    logoPicture = null;
    if (!blob) return;
    let current = true;
    request({ area: "Library", request: { MediaThumbnail: { blob, name: "logo.png", width: 400 } } }, "Thumbnail")
      .then((url) => current && (logoPicture = url))
      .catch(() => {});
    return () => {
      current = false;
    };
  });

  // The default idle screen as this brand draws it, redrawn a moment after
  // the last change.
  let idlePicture: string | null = $state(null);
  $effect(() => {
    const drawn: Brand = JSON.parse(JSON.stringify(brand));
    let current = true;
    const timer = setTimeout(() => {
      loadIdlePicture("Default", 400, drawn).then((r) => current && (idlePicture = r?.picture ?? null));
    }, 150);
    return () => {
      current = false;
      clearTimeout(timer);
    };
  });

  async function pickLogo() {
    try {
      const path = onePath(await pickFiles({ title: "Choose your logo", filters: [{ name: "Pictures", extensions: imageExtensions }] }));
      if (!path) return;
      busy = true;
      brand.logo = (await request({ area: "Library", request: { AddMediaFile: { path } } }, "Media")).blob;
      done = false;
    } catch (e) {
      report(e);
    } finally {
      busy = false;
    }
  }

  function setColor(part: "primary" | "accent", hex: string) {
    brand[part] = fromHex(hex) ?? brand[part];
    done = false;
  }

  async function save() {
    try {
      busy = true;
      const next: Brand = { ...JSON.parse(JSON.stringify(brand)), name: brand.name.trim(), text: customText ? brand.text ?? textColor : null };
      live.settings = await request({ area: "Settings", request: { SaveBrand: next } }, "Settings");
      await refreshThemes();
      done = true;
    } catch (e) {
      report(e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="page">
  <h1>Church look</h1>
  <p class="muted intro">
    Tell Lampstand who you are and it makes a theme in your colours and font, and puts your logo on the default idle screen. You can change
    anything later in Themes.
  </p>

  <ChurchShare only={["logo", "theme picture", "theme video"]} />

  <div class="layout">
    <div class="form">
      <Panel title="Your church">
        <label class="field">
          <span>Name</span>
          <input bind:value={brand.name} oninput={() => (done = false)} placeholder="Grace Chapel" autocomplete="organization" />
        </label>
        <div class="field">
          <span>Logo</span>
          <div class="logo-row">
            {#if logoPicture}<img class="logo" src={logoPicture} alt="Your logo" />{/if}
            <Button size="small" icon="image" onclick={pickLogo} disabled={busy}>{brand.logo ? "Choose another…" : "Choose a picture…"}</Button>
            {#if brand.logo}
              <Button variant="ghost" size="small" onclick={() => { brand.logo = null; done = false; }}>Remove</Button>
            {/if}
          </div>
          <span class="faint small">A PNG with a see-through background looks best. Until then, the idle screen shows the Lampstand lamp.</span>
        </div>
      </Panel>

      <Panel title="Logo bug" description="Your logo, small in a lower corner of every screen and the stream, over songs, scripture, slides and media.">
        {#if !brand.logo}
          <p class="faint small">Add your logo above first. The bug uses the same picture.</p>
        {/if}
        <div class="bug-controls" class:off={!brand.logo}>
          <Switch
            label="Show the logo in a corner"
            disabled={!brand.logo}
            checked={brand.bug.enabled && !!brand.logo}
            onchange={(on) => { brand.bug.enabled = on && !!brand.logo; done = false; }}
          />
          <div class="field">
            <span>Corner</span>
            <Segmented options={corners.map((c) => ({ ...c, disabled: brand.logo ? undefined : "Add a logo first" }))} value={brand.bug.corner} onchange={(id) => { brand.bug.corner = id; done = false; }} label="Corner" />
          </div>
          <div class="field">
            <span>Size</span>
            <Segmented options={sizes.map((c) => ({ ...c, disabled: brand.logo ? undefined : "Add a logo first" }))} value={brand.bug.size} onchange={(id) => { brand.bug.size = id; done = false; }} label="Size" />
          </div>
          <label class="field">
            <span>Opacity: {brand.bug.opacity}%</span>
            <input type="range" min="10" max="100" step="5" disabled={!brand.logo} bind:value={brand.bug.opacity} oninput={() => (done = false)} aria-label="Opacity" />
          </label>
          <span class="faint small">Blank covers it, and it is not drawn over an idle screen that is the logo. It stays inside the safe area, and the stage display never shows it. Turn it on or off during a service from Present's More menu.</span>
        </div>
      </Panel>

      <Panel title="Colours and font">
        <div class="colours">
          <label class="field">
            <span>Main colour</span>
            <input type="color" value={toHex(brand.primary)} oninput={(e) => setColor("primary", e.currentTarget.value)} />
            <span class="faint small">The background of slides and the default idle screen.</span>
          </label>
          <label class="field">
            <span>Second colour</span>
            <input type="color" value={toHex(brand.accent)} oninput={(e) => setColor("accent", e.currentTarget.value)} />
            <span class="faint small">Kept with your look for themes you make by hand.</span>
          </label>
          <div class="field">
            <span>Words</span>
            <label class="check">
              <input type="checkbox" bind:checked={customText} onchange={() => (done = false)} />
              Choose the text colour
            </label>
            {#if customText}
              <input
                type="color"
                aria-label="Text colour"
                value={toHex(brand.text ?? textColor)}
                oninput={(e) => { brand.text = fromHex(e.currentTarget.value); done = false; }}
              />
            {:else}
              <span class="faint small">Black or white, whichever is easier to read on your main colour.</span>
            {/if}
          </div>
        </div>
        <label class="field">
          <span>Font</span>
          <input list="brand-fonts" bind:value={brand.font_family} oninput={() => (done = false)} spellcheck="false" placeholder="Noto Sans" />
          <datalist id="brand-fonts">
            {#each bundledFonts as f (f.family)}<option value={f.family}>{f.note}</option>{/each}
          </datalist>
          <span class="faint small">Any font installed on this computer works. Noto Sans and Noto Serif come with Lampstand.</span>
        </label>
      </Panel>

      <Panel title="Share your look" description="One file with your name, logo, colours, font, themes and idle screen, for another computer or campus.">
        <BrandFile />
      </Panel>

      <div class="save">
        <Button variant="primary" onclick={save} disabled={busy || !brand.name.trim()}>
          {live.settings?.brand ? "Update the theme" : "Make my theme"}
        </Button>
        {#if !brand.name.trim()}<span class="faint small">Add your church's name first.</span>{/if}
      </div>
      {#if done}
        <Banner tone="ready">
          <span>Done. “{brand.name.trim()}” is now the default theme{brand.logo ? " and your logo is on the default idle screen" : ""}.</span>
          {#if onthemes}<Button variant="ghost" size="small" onclick={onthemes}>Open Themes</Button>{/if}
        </Banner>
      {/if}
    </div>

    <div class="previews" aria-label="Preview">
      <h2 class="title">Preview</h2>
      {#if theme}
        <figure>
          <ThemeSketch {theme} lines={["Amazing grace! How sweet the sound", "That saved a wretch like me!"]} footer="John Newton · Public domain" />
          <figcaption class="muted">Songs and scripture</figcaption>
        </figure>
      {/if}
      <figure>
        <SlidePreview
          lines={["Amazing grace! How sweet the sound", "That saved a wretch like me!"]}
          brand={{ logo: logoPicture, background: toHex(brand.primary), accent: toHex(brand.accent), bug: brand.bug }}
        />
        <figcaption class="muted">{brand.bug.enabled && brand.logo ? "Logo bug" : "Logo bug (off)"}</figcaption>
      </figure>
      <figure>
        <div class="idle-screen">
          {#if idlePicture}<img src={idlePicture} alt="" />{/if}
        </div>
        <figcaption class="muted">Default idle screen</figcaption>
      </figure>
    </div>
  </div>
</div>

<style>
  .page {
    max-width: 64rem;
    margin: 0 auto;
    padding: 2rem;
  }
  h1 {
    font-size: 1.3rem;
    font-weight: 600;
    letter-spacing: -0.01em;
  }
  .intro {
    margin: 0.3rem 0 1.5rem;
    max-width: 40rem;
    font-size: 0.875rem;
    line-height: 1.6;
  }
  .layout {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 19rem;
    gap: 1.5rem;
    align-items: start;
  }
  .form {
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    margin-top: 0.9rem;
    font-size: 0.85rem;
  }
  .field > span:first-child {
    color: var(--muted);
    font-size: 0.8rem;
  }
  .small {
    font-size: 0.78rem;
  }
  .colours {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(12rem, 1fr));
    gap: 0 1.25rem;
  }
  input[type="color"] {
    width: 4rem;
    height: 2rem;
    padding: 0.1rem;
  }
  .check {
    display: flex;
    gap: 0.4rem;
    align-items: center;
  }
  .logo-row {
    display: flex;
    align-items: center;
    gap: 0.6rem;
  }
  .logo {
    height: 3rem;
    max-width: 8rem;
    object-fit: contain;
    background: #2a303b;
    border-radius: var(--radius);
    padding: 0.2rem;
  }
  .bug-controls {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    margin-top: 0.6rem;
  }
  .bug-controls.off {
    opacity: 0.55;
  }
  input[type="range"] {
    width: 14rem;
  }
  .save {
    display: flex;
    align-items: center;
    gap: 0.8rem;
  }
  .previews {
    position: sticky;
    top: 1rem;
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }
  figure {
    margin: 0;
  }
  figcaption {
    margin-top: 0.3rem;
    font-size: 0.8rem;
    text-align: center;
  }
  .idle-screen {
    aspect-ratio: 16 / 9;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    overflow: hidden;
    background: var(--canvas);
  }
  .idle-screen img {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
</style>
