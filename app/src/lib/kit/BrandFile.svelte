<!-- Export brand / Import brand: the church's name, logo, colours, font,
     themes and idle screen in one file, to set up once and share. Importing
     shows what the file holds and asks before it adds or replaces
     anything. -->
<script lang="ts">
  import { open as pickFiles, save as pickSave } from "@tauri-apps/plugin-dialog";
  import { request, type BrandContents, type BrandImportMode } from "../api";
  import { onePath } from "../media";
  import { live, refreshSettings, refreshThemes, report } from "../state.svelte";
  import Banner from "./Banner.svelte";
  import Button from "./Button.svelte";
  import Segmented from "./Segmented.svelte";

  const extension = "lampstand-brand";
  let busy = $state(false);
  let incoming: { path: string; contents: BrandContents } | null = $state(null);
  let mode: BrandImportMode = $state("AddAlongside");
  let message = $state("");

  async function exportBrand() {
    try {
      const path = await pickSave({
        title: "Export brand",
        defaultPath: `${(live.settings?.brand?.name || "Brand").replace(/[^\w -]+/g, "").trim() || "Brand"}.${extension}`,
        filters: [{ name: "Lampstand brand", extensions: [extension] }],
      });
      if (!path) return;
      busy = true;
      message = await request({ area: "Library", request: { ExportBrand: { path } } }, "Text");
    } catch (e) {
      report(e);
    } finally {
      busy = false;
    }
  }

  async function chooseFile() {
    try {
      const path = onePath(await pickFiles({ title: "Import brand", filters: [{ name: "Lampstand brand", extensions: [extension] }] }));
      if (!path) return;
      busy = true;
      message = "";
      const contents = await request({ area: "Library", request: { InspectBrand: { path } } }, "BrandFile");
      incoming = { path, contents };
      mode = live.themes.length > 1 ? "AddAlongside" : "Replace";
    } catch (e) {
      report(e);
    } finally {
      busy = false;
    }
  }

  async function importBrand() {
    if (!incoming) return;
    try {
      busy = true;
      message = await request({ area: "Library", request: { ImportBrand: { path: incoming.path, mode } } }, "Text");
      incoming = null;
      await Promise.all([refreshThemes(), refreshSettings()]);
    } catch (e) {
      report(e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="brand-file">
  <div class="buttons">
    <Button size="small" icon="upload" onclick={exportBrand} disabled={busy} title="Saves your name, logo, colours, font, themes and idle screen as one file">Export brand</Button>
    <Button size="small" icon="download" onclick={chooseFile} disabled={busy} title="Reads a brand file from another computer or campus">Import brand…</Button>
  </div>

  {#if incoming}
    {@const c = incoming.contents}
    <section class="confirm" aria-label="Import brand">
      <h3>{c.name ? `Import “${c.name}”?` : "Import this brand file?"}</h3>
      <ul>
        {#if c.name}<li>Church: <strong>{c.name}</strong>{c.has_logo ? ", with its logo" : ""}</li>{/if}
        <li>
          {c.themes.length === 0 ? "No themes" : `${c.themes.length} theme${c.themes.length === 1 ? "" : "s"}: ${c.themes.join(", ")}`}
        </li>
        {#if c.idle}<li>Idle screen: {c.idle}</li>{/if}
        {#if c.font_family}<li>Font: {c.font_family}</li>{/if}
      </ul>
      {#if c.missing_fonts.length}
        <p class="warn">
          {c.missing_fonts.join(", ")} {c.missing_fonts.length === 1 ? "is" : "are"} not installed on this computer. Fonts aren't in the file, so
          Lampstand will use a stand-in until you install {c.missing_fonts.length === 1 ? "it" : "them"}.
        </p>
      {/if}
      <Segmented
        label="How to import"
        value={mode}
        onchange={(m) => (mode = m)}
        options={[
          { id: "AddAlongside", label: "Add alongside" },
          { id: "Replace", label: "Replace" },
        ]}
      />
      <p class="muted small">
        {#if mode === "AddAlongside"}
          Adds the themes next to yours, renaming any with the same name. Your saved church name and idle screen are only filled in if you have none.
        {:else}
          A theme replaces the one of the same name here, and is added if there is none. The Starter theme is never replaced. Your saved church name, logo and idle screen are replaced.
        {/if}
      </p>
      <div class="buttons">
        <Button variant="primary" size="small" onclick={importBrand} disabled={busy}>{mode === "Replace" ? "Replace" : "Add"}</Button>
        <Button variant="ghost" size="small" onclick={() => (incoming = null)}>Cancel</Button>
      </div>
    </section>
  {/if}

  {#if message}
    <Banner tone="ready"><span>{message}</span></Banner>
  {/if}
</div>

<style>
  .brand-file {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    font-size: 0.85rem;
  }
  .buttons {
    display: flex;
    gap: 0.5rem;
    flex-wrap: wrap;
  }
  .confirm {
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
    padding: 0.9rem 1rem;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-lg);
    background: var(--raised);
  }
  h3 {
    font-size: 0.95rem;
  }
  ul {
    margin: 0;
    padding-left: 1.1rem;
  }
  .warn {
    color: var(--warn);
  }
  .small {
    font-size: 0.8rem;
  }
</style>
