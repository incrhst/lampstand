<!-- The free song database, for a library with no songs yet: a language,
     "Add songs", and the import's progress while it downloads. -->
<script lang="ts">
  import type { SongDatabase } from "../../lib/api";
  import { live } from "../../lib/state.svelte";
  import { songProgress } from "../../lib/library";
  import Button from "../../lib/kit/Button.svelte";
  import Icon from "../../lib/kit/Icon.svelte";
  import ProgressBar from "../../lib/kit/ProgressBar.svelte";

  let {
    databases,
    code = $bindable(),
    downloading,
    disabled,
    ondownload,
  }: {
    databases: SongDatabase[];
    /** The chosen database's code ("en"). */
    code: string;
    downloading: boolean;
    /** Another import is running. */
    disabled: boolean;
    ondownload: () => void;
  } = $props();

  let source = $derived(databases.find((d) => d.code === code)?.source ?? "worshipleaderapp.com");
  let language = $derived(databases.find((d) => d.code === code)?.name ?? "");
  /** The status bar's line while it is about this download, not an older message. */
  let step = $derived(/^(Downloading|Reading|Adding songs)/.test(live.status) ? live.status : `Adding ${language} songs…`);
</script>

<section class="card" aria-labelledby="song-db-title">
  <div class="head">
    <span class="badge"><Icon name="song" /></span>
    <div>
      <h3 id="song-db-title">Free song database</h3>
      <p class="muted">Worship songs from {source}, updated daily. Songs you already have are skipped.</p>
    </div>
  </div>

  {#if downloading}
    <div class="progress" role="status" aria-live="polite">
      <span class="muted">{step}</span>
      <ProgressBar value={songProgress(live.status)} label="Adding {language} songs" />
    </div>
  {:else}
    <div class="row">
      <select aria-label="Song database language" bind:value={code}>
        {#each databases as d (d.code)}
          <option value={d.code}>{d.name}</option>
        {/each}
      </select>
      <Button variant="primary" size="small" icon="download" onclick={ondownload} disabled={disabled || databases.length === 0}>
        Add songs
      </Button>
    </div>
  {/if}

  <p class="note">Many songs are under copyright. Your church's CCLI (or similar) licence covers projecting them.</p>
</section>

<style>
  .card {
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    padding: 0.9rem;
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }
  .head {
    display: flex;
    gap: 0.75rem;
  }
  .badge {
    width: 2.1rem;
    height: 2.1rem;
    flex-shrink: 0;
    border-radius: var(--radius);
    background: var(--gold-soft);
    color: var(--gold);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  h3 {
    font-size: 0.875rem;
    font-weight: 600;
  }
  .head p {
    margin-top: 0.15rem;
    font-size: 0.78rem;
    line-height: 1.45;
  }
  .row {
    display: flex;
    gap: 0.5rem;
  }
  .row select {
    flex: 1;
    min-width: 0;
    padding: 0.3rem 0.5rem;
  }
  .row :global(button) {
    flex: 1;
  }
  .progress {
    display: flex;
    flex-direction: column;
    gap: 0.45rem;
    font-size: 0.78rem;
  }
  .note {
    font-size: 0.72rem;
    line-height: 1.45;
    color: var(--faint);
  }
</style>
