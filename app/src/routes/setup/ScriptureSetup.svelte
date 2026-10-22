<!-- Setup › Scripture: the Bible versions on this device, and which one a
     reference is read from when it does not name one. "James 3:11" uses the
     default; "James 3:11 ESV" uses the ESV if this device has it. Versions
     are imported in Plan (drop a Bible file on the library). -->
<script lang="ts">
  import { request, type Translation } from "../../lib/api";
  import { chooseTranslation, versionLabel } from "../../lib/bibles";
  import { loadDefaultTranslation, saveDefaultTranslation } from "../../lib/bibleSetting";
  import { report } from "../../lib/state.svelte";
  import Panel from "../../lib/kit/Panel.svelte";

  let translations: Translation[] = $state([]);
  let chosen = $state("");
  let loaded = $state(false);

  Promise.all([request({ area: "Library", request: "ListTranslations" }, "Translations"), loadDefaultTranslation()])
    .then(([all, saved]) => {
      translations = [...all].sort((a, b) => a.name.localeCompare(b.name));
      chosen = chooseTranslation(translations, saved);
      loaded = true;
    })
    .catch(report);

  function pick() {
    saveDefaultTranslation(chosen).catch(report);
  }

  let example = $derived(translations.find((t) => t.id !== chosen) ?? translations[0]);
</script>

<div class="page">
  <h1>Scripture</h1>
  <p class="muted intro">
    Type a reference to read it from the Bible you choose here. Add a version after it to read from another one.
  </p>

  <Panel title="Default version" description="Used when a reference names no version.">
    {#if translations.length === 0}
      {#if loaded}
        <p class="muted">No Bible is on this device yet. Drop a Bible file on the library in Plan to import one.</p>
      {/if}
    {:else}
      <select aria-label="Default Bible version" bind:value={chosen} onchange={pick}>
        {#each translations as t (t.id)}
          <option value={t.id}>{t.abbreviation ? `${t.abbreviation} · ${t.name}` : t.name}</option>
        {/each}
      </select>
      <p class="muted small">
        <strong>James 3:11</strong> is read from {versionLabel(translations.find((t) => t.id === chosen) ?? translations[0])}.
        {#if example && example.id !== chosen}
          <strong>James 3:11 {versionLabel(example)}</strong> is read from {versionLabel(example)}.
        {/if}
      </p>
    {/if}
  </Panel>

  <Panel title="Versions on this device" description="A version that is not here is reported as unrecognised, with this list.">
    {#if translations.length === 0}
      <p class="muted">None.</p>
    {:else}
      <ul class="versions">
        {#each translations as t (t.id)}
          <li>
            <strong>{versionLabel(t)}</strong>
            <span class="muted">{t.abbreviation ? t.name : ""}{t.licence === "PublicDomain" ? " · public domain" : " · licensed"}</span>
          </li>
        {/each}
      </ul>
    {/if}
    <p class="muted small">
      Lampstand includes only public-domain Bibles. A licensed version such as the ESV is available once your church
      imports it, under your church's licence.
    </p>
  </Panel>
</div>

<style>
  .page :global(section + section) {
    margin-top: 1rem;
  }
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
  .small {
    margin-top: 0.75rem;
    font-size: 0.8125rem;
  }
  .versions {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    font-size: 0.875rem;
  }
</style>
