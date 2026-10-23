<!-- Setup › Themes: how songs, scripture and text slides look. The list of
     the library's themes on the left (new, duplicate, rename, delete), one
     theme's settings on the right, and a live preview drawn by the same
     layout and renderer the screens use, at the main screen's shape. A
     theme saves as it is changed. -->
<script lang="ts">
  import { open as pickFiles } from "@tauri-apps/plugin-dialog";
  import { request, type Theme, type ThemeSample } from "../../lib/api";
  import { imageExtensions, onePath, videoExtensions } from "../../lib/media";
  import { live, mainOutput, refreshSettings, refreshThemes, report, saveSettings } from "../../lib/state.svelte";
  import { targetSize } from "../../lib/screenShape";
  import {
    STARTER_THEME_ID,
    aligns,
    backgroundColor,
    backgroundKind,
    bundledFonts,
    cannotDelete,
    defaultThemeId,
    directions,
    duplicate,
    fromHex,
    makeTransition,
    readability,
    textAreaId,
    textAreas,
    toHex,
    transitionDirection,
    transitionName,
    transitions,
    valigns,
    weights,
    type BackgroundKind,
    type DirectionName,
    type TransitionName,
  } from "../../lib/themes";
  import Button from "../../lib/kit/Button.svelte";
  import Banner from "../../lib/kit/Banner.svelte";
  import Icon from "../../lib/kit/Icon.svelte";
  import ListRow from "../../lib/kit/ListRow.svelte";
  import Panel from "../../lib/kit/Panel.svelte";
  import Segmented from "../../lib/kit/Segmented.svelte";
  import Switch from "../../lib/kit/Switch.svelte";
  import ThemeSketch from "../../lib/kit/ThemeSketch.svelte";
  import BrandFile from "../../lib/kit/BrandFile.svelte";

  let { onbrand }: { onbrand?: () => void } = $props();

  let selectedId: string | null = $state(null);
  let draft = $state<Theme | null>(null);
  let renaming = $state(false);
  let confirmDelete = $state(false);
  let saved = $state(true);

  // Sample text for the preview and for the sketch when there is no picture.
  const samples: { id: ThemeSample; label: string; lines: string[]; footer: string }[] = [
    {
      id: "Lyrics",
      label: "Song",
      lines: ["Amazing grace! How sweet the sound", "That saved a wretch like me!", "I once was lost, but now am found", "Was blind, but now I see."],
      footer: "John Newton · Public domain",
    },
    {
      id: "Verse",
      label: "Scripture",
      lines: ["For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life."],
      footer: "John 3:16 KJV",
    },
    { id: "Text", label: "Text slide", lines: ["Welcome", "Coffee and tea after the service in the hall"], footer: "" },
  ];

  /** The shape of the main screen, so the preview is what that screen will show. */
  let shape = $derived.by(() => {
    const main = mainOutput(live.outputs);
    return main ? targetSize(main.target) : { width: 1920, height: 1080 };
  });

  let defaultId = $derived(defaultThemeId(live.settings, live.themes));
  let selected = $derived(live.themes.find((t) => t.id === selectedId) ?? null);
  let hint = $derived(draft ? readability(draft) : null);
  let areaId = $derived(draft ? textAreaId(draft.text_area) : "full");
  let blocked = $derived(draft ? cannotDelete(draft, live.themes) : null);

  refreshThemes();
  refreshSettings();

  // Open the first theme (the default's) once the list arrives.
  $effect(() => {
    if (selectedId === null && live.themes.length) select(defaultId);
  });

  function select(id: string) {
    flush();
    const theme = live.themes.find((t) => t.id === id);
    selectedId = id;
    draft = theme ? (JSON.parse(JSON.stringify(theme)) as Theme) : null;
    renaming = false;
    confirmDelete = false;
    saved = true;
    queuePreview(0);
  }

  // Saving waits for a pause in typing; leaving the page or the theme saves at once.
  let saveTimer: ReturnType<typeof setTimeout> | undefined;
  async function save() {
    clearTimeout(saveTimer);
    saveTimer = undefined;
    if (!draft) return;
    const theme = JSON.parse(JSON.stringify(draft)) as Theme;
    try {
      await request({ area: "Library", request: { PutTheme: theme } }, "Ok");
      live.themes = live.themes.map((t) => (t.id === theme.id ? theme : t));
      if (draft?.id === theme.id) saved = true;
    } catch (e) {
      report(e);
    }
  }
  function flush() {
    if (saveTimer) save();
  }
  $effect(() => () => flush());

  /** Called after every change to the draft. */
  function changed() {
    saved = false;
    clearTimeout(saveTimer);
    saveTimer = setTimeout(save, 500);
    queuePreview(250);
  }

  // The preview: one picture per sample, drawn by the core.
  let pictures: Partial<Record<ThemeSample, string>> = $state({});
  let previewFailed = $state(false);
  let previewTimer: ReturnType<typeof setTimeout> | undefined;
  let previewSeq = 0;
  function queuePreview(delay: number) {
    clearTimeout(previewTimer);
    previewTimer = setTimeout(drawPreview, delay);
  }
  async function drawPreview() {
    if (!draft) return;
    const seq = ++previewSeq;
    const theme = JSON.parse(JSON.stringify(draft)) as Theme;
    try {
      const drawn = await Promise.all(
        samples.map(async (s) => [
          s.id,
          await request(
            {
              area: "Library",
              request: { ThemePreview: { theme, sample: s.id, width: shape.width, height: shape.height, thumb_width: 640 } },
            },
            "Thumbnail",
          ),
        ] as const),
      );
      if (seq !== previewSeq) return;
      pictures = Object.fromEntries(drawn);
      previewFailed = false;
    } catch {
      if (seq === previewSeq) previewFailed = true;
    }
  }
  $effect(() => () => clearTimeout(previewTimer));
  // A different main screen shape redraws the preview.
  $effect(() => {
    shape.width;
    shape.height;
    queuePreview(0);
  });

  async function create() {
    const base = live.themes.find((t) => t.id === STARTER_THEME_ID) ?? live.themes[0];
    if (!base) return;
    const theme = { ...duplicate(base, live.themes, crypto.randomUUID()), name: uniqueNew() };
    await add(theme);
  }
  function uniqueNew(): string {
    const taken = new Set(live.themes.map((t) => t.name.toLowerCase()));
    if (!taken.has("new theme")) return "New theme";
    for (let n = 2; ; n++) if (!taken.has(`new theme ${n}`)) return `New theme ${n}`;
  }
  async function copy() {
    if (!draft) return;
    await add(duplicate(draft, live.themes, crypto.randomUUID()));
  }
  async function add(theme: Theme) {
    flush();
    try {
      await request({ area: "Library", request: { PutTheme: theme } }, "Ok");
      live.themes = [...live.themes, theme];
      await refreshThemes();
      select(theme.id);
      renaming = true;
    } catch (e) {
      report(e);
    }
  }

  async function remove() {
    if (!draft || blocked) return;
    if (!confirmDelete) {
      confirmDelete = true;
      return;
    }
    const id = draft.id;
    try {
      clearTimeout(saveTimer);
      saveTimer = undefined;
      await request({ area: "Library", request: { DeleteTheme: id } }, "Ok");
      selectedId = null;
      draft = null;
      await Promise.all([refreshThemes(), refreshSettings()]);
      live.status = "Theme deleted. Services and screens that used it now use the default.";
    } catch (e) {
      report(e);
    }
  }

  async function makeDefault() {
    if (!draft) return;
    await saveSettings({ default_theme: draft.id === STARTER_THEME_ID ? null : draft.id });
  }

  async function restoreStarter() {
    try {
      await request({ area: "Library", request: "RestoreStarterTheme" }, "Ok");
      await refreshThemes();
      selectedId = null;
      select(STARTER_THEME_ID);
      live.status = "The Starter theme is back as it came.";
    } catch (e) {
      report(e);
    }
  }

  // Background: a colour, or a picture or video copied into the library.
  async function setBackground(kind: BackgroundKind) {
    if (!draft) return;
    if (kind === "None") draft.background = "None";
    else if (kind === "Color") draft.background = { Color: backgroundColor(draft.background) };
    else {
      const video = kind === "Video";
      const path = onePath(
        await pickFiles({
          title: video ? "Choose a background video" : "Choose a background picture",
          filters: [{ name: video ? "Video" : "Pictures", extensions: video ? videoExtensions : imageExtensions }],
        }),
      );
      if (!path) return;
      try {
        live.status = "Copying into the library…";
        const file = await request({ area: "Library", request: { AddMediaFile: { path } } }, "Media");
        draft.background = video ? { Video: file.blob } : { Image: file.blob };
        live.status = `Added ${file.name}`;
      } catch (e) {
        report(e);
        return;
      }
    }
    changed();
  }

  function setTransition(name: TransitionName, direction: DirectionName, ms: number) {
    if (!draft) return;
    draft.transition = makeTransition(name, direction, ms);
    changed();
  }

  function setArea(id: string) {
    if (!draft) return;
    const preset = textAreas.find((t) => t.id === id);
    if (preset) draft.text_area = { ...preset.rect };
    changed();
  }

  /** A percentage field of the text area. */
  function setAreaPart(part: "x" | "y" | "width" | "height", percent: number | null) {
    if (!draft || percent === null || Number.isNaN(percent)) return;
    const v = Math.min(100, Math.max(0, percent)) / 100;
    draft.text_area = { ...draft.text_area, [part]: v };
    const a = draft.text_area;
    // Stay inside the screen.
    if (a.x + a.width > 1) a.width = 1 - a.x;
    if (a.y + a.height > 1) a.height = 1 - a.y;
    changed();
  }

  function setSize(value: number | null) {
    if (!draft || !value) return;
    draft.text.size = Math.min(400, Math.max(12, value));
    if (draft.text.min_size > draft.text.size) draft.text.min_size = draft.text.size;
    changed();
  }
  function setMinSize(value: number | null) {
    if (!draft || !value) return;
    draft.text.min_size = Math.min(draft.text.size, Math.max(8, value));
    changed();
  }

  const percent = (v: number) => Math.round(v * 1000) / 10;
  const isDefault = $derived(!!draft && draft.id === defaultId);
  const kinds = [
    { id: "None", label: "None" },
    { id: "Color", label: "Colour" },
    { id: "Image", label: "Picture" },
    { id: "Video", label: "Video" },
  ] as const;
</script>

<div class="themes">
  <aside class="list" aria-label="Themes">
    <div class="list-head">
      <h2 class="title">Themes</h2>
      <Button size="small" icon="plus" onclick={create}>New</Button>
    </div>
    <ul class="plain list-body">
      {#each live.themes as t (t.id)}
        <ListRow
          title={t.name}
          subtitle={t.id === defaultId ? "Default · " + t.text.font_family : t.text.font_family}
          active={t.id === selectedId}
          onclick={() => select(t.id)}
        >
          {#snippet leading()}
            <span
              class="swatch"
              style="background:{backgroundKind(t.background) === 'Color' ? toHex(backgroundColor(t.background)) : '#000'};color:{toHex(t.text.color)}"
              aria-hidden="true">Aa</span
            >
          {/snippet}
        </ListRow>
      {/each}
    </ul>
    {#if onbrand}
      <div class="brand-link">
        <Button variant="ghost" size="small" icon="palette" onclick={onbrand}>Set up from your church's colours…</Button>
      </div>
    {/if}
  </aside>

  <div class="editor">
    {#if draft}
      <div class="page">
        <div class="title-row">
          {#if renaming}
            <input
              class="name"
              aria-label="Theme name"
              bind:value={draft.name}
              oninput={changed}
              onblur={() => (renaming = false)}
              onkeydown={(e) => (e.key === "Enter" || e.key === "Escape") && (renaming = false)}
            />
          {:else}
            <h1>{draft.name}</h1>
            <Button variant="ghost" size="small" onclick={() => (renaming = true)}>Rename</Button>
          {/if}
          <span class="faint saved" aria-live="polite">{saved ? "Saved" : "Saving…"}</span>
        </div>
        <div class="actions">
          <Button size="small" icon="copy" onclick={copy}>Duplicate</Button>
          <Button size="small" icon="star" onclick={makeDefault} disabled={isDefault} title="Used for services and screens that don't choose a theme">
            {isDefault ? "This is the default" : "Use as the default"}
          </Button>
          {#if draft.id === STARTER_THEME_ID}
            <Button size="small" onclick={restoreStarter} title="Puts the Starter theme back exactly as it came">Reset to original</Button>
          {/if}
          <Button
            variant={confirmDelete ? "danger" : "ghost"}
            size="small"
            icon="trash"
            disabled={!!blocked}
            title={blocked ?? "Services and screens using it go back to the default"}
            onclick={remove}>{confirmDelete ? "Click again to delete" : "Delete"}</Button
          >
        </div>

        <section class="preview" aria-label="Preview">
          <div class="previews">
            {#each samples as s (s.id)}
              <figure>
                <div class="frame" style="aspect-ratio:{shape.width}/{shape.height}">
                  {#if pictures[s.id] && !previewFailed}
                    <img src={pictures[s.id]} alt="{s.label} sample" />
                  {:else}
                    <ThemeSketch theme={draft} lines={s.lines} footer={s.footer} width={shape.width} height={shape.height} />
                  {/if}
                </div>
                <figcaption class="muted">{s.label}</figcaption>
              </figure>
            {/each}
          </div>
          <p class="faint small">
            Drawn at the shape of {mainOutput(live.outputs)?.name ?? "a 16:9 screen"} ({shape.width}×{shape.height}){previewFailed
              ? ". A sketch is shown: the exact picture isn't available on this computer."
              : "."}
          </p>
        </section>

        {#if hint}
          <div class="gap"><Banner tone="warn"><span>{hint} Try a lighter or darker text colour.</span></Banner></div>
        {/if}

        <Panel title="Text" description="Sizes are in pixels on a 1080-pixel-tall screen, and scale to every screen. Long verses shrink to fit, down to the smallest size.">
          <div class="grid">
            <label>
              <span>Font</span>
              <input
                list="theme-fonts"
                bind:value={draft.text.font_family}
                oninput={changed}
                spellcheck="false"
                placeholder="Noto Sans"
              />
              <datalist id="theme-fonts">
                {#each bundledFonts as f (f.family)}<option value={f.family}>{f.note}</option>{/each}
              </datalist>
            </label>
            <label>
              <span>Weight</span>
              <select
                value={draft.text.weight}
                onchange={(e) => {
                  draft!.text.weight = Number(e.currentTarget.value);
                  changed();
                }}
              >
                {#each weights as w (w.id)}<option value={w.id}>{w.label}</option>{/each}
              </select>
            </label>
            <label>
              <span>Size</span>
              <input type="number" min="12" max="400" value={draft.text.size} onchange={(e) => setSize(e.currentTarget.valueAsNumber)} />
            </label>
            <label>
              <span>Smallest size</span>
              <input type="number" min="8" max="400" value={draft.text.min_size} onchange={(e) => setMinSize(e.currentTarget.valueAsNumber)} />
            </label>
            <label class="colour">
              <span>Text colour</span>
              <input
                type="color"
                value={toHex(draft.text.color)}
                oninput={(e) => {
                  draft!.text.color = fromHex(e.currentTarget.value) ?? draft!.text.color;
                  changed();
                }}
              />
            </label>
            <div class="field">
              <span>Style</span>
              <Switch checked={draft.text.italic} onchange={(on) => { draft!.text.italic = on; changed(); }} label="Italic" />
            </div>
          </div>
          <div class="grid segs">
            <div class="field">
              <span>Alignment</span>
              <Segmented label="Alignment" options={aligns} value={draft.text.align === "Justify" ? "Left" : draft.text.align} onchange={(id) => { draft!.text.align = id; changed(); }} />
            </div>
            <div class="field">
              <span>Vertical position</span>
              <Segmented label="Vertical alignment" options={valigns} value={draft.text.valign} onchange={(id) => { draft!.text.valign = id; changed(); }} />
            </div>
          </div>
        </Panel>

        <Panel title="Background" description="A picture or video is copied into the library and fills the whole screen behind the text.">
          <Segmented label="Background" options={kinds} value={backgroundKind(draft.background) as BackgroundKind} onchange={setBackground} />
          {#if backgroundKind(draft.background) === "Color"}
            <label class="inline colour">
              <span>Background colour</span>
              <input
                type="color"
                value={toHex(backgroundColor(draft.background))}
                oninput={(e) => {
                  draft!.background = { Color: fromHex(e.currentTarget.value) ?? backgroundColor(draft!.background) };
                  changed();
                }}
              />
            </label>
          {:else if backgroundKind(draft.background) === "Image" || backgroundKind(draft.background) === "Video"}
            <p class="muted small">
              <Icon name={backgroundKind(draft.background) === "Image" ? "image" : "media"} size={14} />
              A {backgroundKind(draft.background) === "Image" ? "picture" : "video"} from the library.
              <Button variant="ghost" size="small" onclick={() => setBackground(backgroundKind(draft!.background))}>Choose another…</Button>
            </p>
          {:else if backgroundKind(draft.background) === "None"}
            <p class="muted small">Nothing is drawn: black on a screen, see-through on a picture sent to OBS.</p>
          {/if}
        </Panel>

        <Panel title="Layout" description="Where the words go, and how many lines a song puts on a slide.">
          <div class="grid">
            <div class="field wide">
              <span>Text area</span>
              <Segmented
                label="Text area"
                options={[...textAreas.map((t) => ({ id: t.id, label: t.label })), { id: "custom", label: "Custom" }]}
                value={areaId}
                onchange={setArea}
              />
            </div>
            {#each [["x", "Left"], ["y", "Top"], ["width", "Width"], ["height", "Height"]] as const as [part, label] (part)}
              <label>
                <span>{label} (%)</span>
                <input type="number" min="0" max="100" step="1" value={percent(draft.text_area[part])} onchange={(e) => setAreaPart(part, e.currentTarget.valueAsNumber)} />
              </label>
            {/each}
            <label>
              <span>Lines per slide</span>
              <input
                type="number"
                min="1"
                max="12"
                value={draft.max_lines}
                onchange={(e) => {
                  const n = e.currentTarget.valueAsNumber;
                  if (!n) return;
                  draft!.max_lines = Math.min(12, Math.max(1, Math.round(n)));
                  changed();
                }}
              />
            </label>
            <div class="field">
              <span>Song footer</span>
              <Switch checked={draft.show_copyright} onchange={(on) => { draft!.show_copyright = on; changed(); }} label="Show copyright" />
            </div>
          </div>
        </Panel>

        <Panel title="Changing slides" description="How one slide gives way to the next on the screens.">
          <div class="grid">
            <label>
              <span>Transition</span>
              <select
                value={transitionName(draft.transition.kind)}
                onchange={(e) =>
                  setTransition(e.currentTarget.value as TransitionName, transitionDirection(draft!.transition.kind), draft!.transition.duration_ms || 400)}
              >
                {#each transitions as t (t.id)}<option value={t.id}>{t.label}</option>{/each}
              </select>
            </label>
            {#if transitions.find((t) => t.id === transitionName(draft!.transition.kind) && "sided" in t)}
              <label>
                <span>Direction</span>
                <select
                  value={transitionDirection(draft.transition.kind)}
                  onchange={(e) =>
                    setTransition(transitionName(draft!.transition.kind), e.currentTarget.value as DirectionName, draft!.transition.duration_ms)}
                >
                  {#each directions as d (d.id)}<option value={d.id}>{d.label}</option>{/each}
                </select>
              </label>
            {/if}
            {#if transitionName(draft.transition.kind) !== "Cut"}
              <label>
                <span>Length (ms)</span>
                <input
                  type="number"
                  min="50"
                  max="3000"
                  step="50"
                  value={draft.transition.duration_ms}
                  onchange={(e) =>
                    setTransition(transitionName(draft!.transition.kind), transitionDirection(draft!.transition.kind), e.currentTarget.valueAsNumber || 400)}
                />
              </label>
            {/if}
          </div>
        </Panel>

        <Panel title="Share your look" description="Export every theme, your logo, colours and idle screen as one brand file, or import one from another computer or campus.">
          <BrandFile />
        </Panel>
      </div>
    {:else}
      <div class="page">
        <p class="muted">Choose a theme, or make a new one.</p>
        <Panel title="Share your look" description="Export every theme, your logo, colours and idle screen as one brand file, or import one from another computer or campus.">
          <BrandFile />
        </Panel>
      </div>
    {/if}
  </div>
</div>

<style>
  .themes {
    display: grid;
    grid-template-columns: 15.5rem minmax(0, 1fr);
    height: 100%;
  }
  .list {
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-right: 1px solid var(--line);
  }
  .list-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 1.25rem 1rem 0.75rem;
  }
  .list-body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0 0.5rem 0.5rem;
  }
  .brand-link {
    padding: 0.5rem 0.75rem 1rem;
    border-top: 1px solid var(--line);
  }
  .swatch {
    display: grid;
    place-items: center;
    width: var(--tile-width);
    height: var(--tile-height);
    border: 1px solid var(--line-strong);
    border-radius: 4px;
    font-weight: 600;
    font-size: 0.85rem;
  }
  .editor {
    min-width: 0;
    min-height: 0;
    overflow-y: auto;
  }
  .page {
    max-width: 56rem;
    margin: 0 auto;
    padding: 1.5rem 2rem 3rem;
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }
  .title-row {
    display: flex;
    align-items: center;
    gap: 0.75rem;
  }
  h1 {
    font-size: 1.3rem;
    font-weight: 600;
    letter-spacing: -0.01em;
  }
  .name {
    font-size: 1.1rem;
    min-width: 16rem;
  }
  .saved {
    margin-left: auto;
    font-size: 0.8rem;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
  }
  .previews {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 0.75rem;
  }
  figure {
    margin: 0;
  }
  .frame {
    width: 100%;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    overflow: hidden;
    background: #000;
  }
  .frame img {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  figcaption {
    margin-top: 0.3rem;
    font-size: 0.8rem;
    text-align: center;
  }
  .small {
    font-size: 0.8rem;
    margin-top: 0.4rem;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(11rem, 1fr));
    gap: 0.9rem 1rem;
    margin-top: 0.9rem;
  }
  .grid.segs {
    grid-template-columns: repeat(auto-fill, minmax(16rem, 1fr));
  }
  .grid label,
  .field {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    font-size: 0.85rem;
  }
  .grid label > span,
  .field > span {
    color: var(--muted);
    font-size: 0.8rem;
  }
  .wide {
    grid-column: 1 / -1;
  }
  .colour input[type="color"] {
    width: 4rem;
    height: 2rem;
    padding: 0.1rem;
  }
  .inline {
    display: inline-flex;
    align-items: center;
    gap: 0.6rem;
    margin-top: 0.9rem;
    font-size: 0.85rem;
  }
  .gap {
    margin: 0;
  }
  p.small {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    flex-wrap: wrap;
  }
  .preview p.small {
    display: block;
  }
</style>
