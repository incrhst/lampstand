<!-- CCLI SongSelect, for a church that has a licence: download songs there
     as usual and they are added here as they land in the Downloads folder. -->
<script lang="ts">
  import Button from "../../lib/kit/Button.svelte";
  import Icon from "../../lib/kit/Icon.svelte";

  let {
    watching,
    disabled,
    ontoggle,
  }: {
    watching: boolean;
    /** Another import is running. */
    disabled: boolean;
    ontoggle: () => void;
  } = $props();
</script>

<section class="card" aria-labelledby="song-select-title">
  <div class="head">
    <span class="badge"><Icon name="song" /></span>
    <div>
      <h3 id="song-select-title">CCLI SongSelect</h3>
      {#if watching}
        <p class="muted" role="status" aria-live="polite">
          Watching your Downloads folder. Download a song's <strong>Lyrics</strong> (.txt) or <strong>USR</strong> file from
          songselect.ccli.com and it is added here.
        </p>
      {:else}
        <p class="muted">
          Songs you download from songselect.ccli.com (USR or lyrics files) are added as they land in your Downloads folder.
        </p>
      {/if}
    </div>
  </div>
  <Button variant={watching ? "secondary" : "primary"} size="small" icon={watching ? "check" : "download"} onclick={ontoggle} {disabled}>
    {watching ? "Stop watching" : "Watch Downloads"}
  </Button>
  <p class="note">Needs your church's CCLI SongSelect subscription. Songs already in the library, matched by CCLI number, are skipped.</p>
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
  .note {
    font-size: 0.72rem;
    line-height: 1.45;
    color: var(--faint);
  }
</style>
