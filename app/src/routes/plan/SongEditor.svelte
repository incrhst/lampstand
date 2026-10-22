<!-- Edits one song: its details, its sections (label and lines) and the order
     it is sung in by default. Lines are kept as typed, so chords written
     inline as [G]word stay exactly where they are. Saving writes the song
     back; a service that has the song open picks the change up. -->
<script lang="ts">
  import { request, send } from "../../lib/api";
  import { live, report } from "../../lib/state.svelte";
  import { moved, newSection, nextLabel, problems, toDraft, toSong, type SongDraft } from "../../lib/songEdit";
  import type { Song } from "../../lib/api";
  import Button from "../../lib/kit/Button.svelte";
  import SongOrder from "./SongOrder.svelte";

  let { song: id, onclose }: { song: string; onclose: () => void } = $props();

  let original = $state<Song | null>(null);
  let draft = $state<SongDraft | null>(null);
  let saving = $state(false);
  let failed = $state("");

  $effect(() => {
    request({ area: "Library", request: { GetSong: id } }, "Song")
      .then((song) => {
        original = song;
        draft = toDraft(song);
      })
      .catch((e) => {
        report(e);
        onclose();
      });
  });

  let found = $derived(draft ? problems(draft) : []);
  let labels = $derived(draft?.sections.map((s) => s.label.trim()) ?? []);

  async function save() {
    if (!original || !draft || found.length > 0 || saving) return;
    saving = true;
    failed = "";
    try {
      await send({ area: "Library", request: { PutSong: toSong(original, draft) } });
      live.songsVersion++;
      live.status = `Saved ${draft.title.trim()}`;
      onclose();
    } catch (e) {
      failed = e instanceof Error ? e.message : String(e);
    } finally {
      saving = false;
    }
  }

  function onkey(e: KeyboardEvent) {
    if (e.key === "Escape") onclose();
    else if (e.key === "s" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      save();
    }
  }
</script>

<svelte:window onkeydown={onkey} />

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="backdrop" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog" role="dialog" aria-modal="true" aria-label="Edit song">
    {#if draft}
      <header>
        <h2>Edit song</h2>
        <Button variant="ghost" size="small" icon="x" label="Close" onclick={onclose} />
      </header>
      <div class="body">
        <div class="fields">
          <label class="wide">Title<input type="text" bind:value={draft.title} /></label>
          <label>Author<input type="text" bind:value={draft.author} /></label>
          <label>Copyright<input type="text" bind:value={draft.copyright} /></label>
          <label>CCLI number<input type="text" inputmode="numeric" bind:value={draft.ccli} /></label>
          <label>Key<input type="text" bind:value={draft.key} placeholder="G" /></label>
          <label>Tempo (bpm)<input type="text" inputmode="numeric" bind:value={draft.tempo} /></label>
        </div>

        <div class="order">
          <span class="caption">Order</span>
          <SongOrder
            bare
            sections={labels}
            songOrder={[]}
            value={draft.arrangement.length > 0 ? draft.arrangement : null}
            label="Default order"
            onchange={(order) => draft && (draft.arrangement = order ?? [])}
          />
          <span class="muted small">Blank sings the sections as written. A service can still use its own order.</span>
        </div>

        <h3>Sections</h3>
        <ol class="sections">
          {#each draft.sections as section, i (section.uid)}
            <li>
              <div class="section-head">
                <input type="text" class="label" aria-label="Section label" placeholder="V1" bind:value={section.label} />
                <Button variant="ghost" size="small" icon="prev" label="Move up" disabled={i === 0} onclick={() => draft && (draft.sections = moved(draft.sections, i, -1))} />
                <Button variant="ghost" size="small" icon="next" label="Move down" disabled={i === draft.sections.length - 1} onclick={() => draft && (draft.sections = moved(draft.sections, i, 1))} />
                <Button variant="ghost" size="small" icon="trash" label="Remove section {section.label}" onclick={() => draft && (draft.sections = draft.sections.filter((s) => s !== section))} />
              </div>
              <textarea
                rows={Math.max(3, section.text.split("\n").length)}
                aria-label="Lines of {section.label || 'section'}"
                spellcheck="false"
                bind:value={section.text}
              ></textarea>
            </li>
          {/each}
        </ol>
        <Button icon="plus" onclick={() => draft && (draft.sections = [...draft.sections, newSection(nextLabel(labels))])}>Add section</Button>
      </div>
      <footer>
        <span class="problems" role="status">{failed || found[0] || ""}</span>
        <Button variant="ghost" onclick={onclose}>Cancel</Button>
        <Button variant="primary" onclick={save} disabled={found.length > 0 || saving}>Save</Button>
      </footer>
    {:else}
      <p class="muted loading">Loading…</p>
    {/if}
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.55);
  }
  .dialog {
    width: min(46rem, calc(100vw - 2rem));
    max-height: calc(100vh - 3rem);
    display: flex;
    flex-direction: column;
    background: var(--panel);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-lg);
  }
  header,
  footer {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.75rem 1rem;
  }
  header {
    justify-content: space-between;
    border-bottom: 1px solid var(--line);
  }
  footer {
    border-top: 1px solid var(--line);
  }
  h2,
  h3 {
    margin: 0;
    font-size: 1rem;
  }
  h3 {
    font-size: 0.875rem;
    margin: 1.25rem 0 0.5rem;
  }
  .body {
    overflow-y: auto;
    padding: 1rem;
  }
  .fields {
    display: grid;
    grid-template-columns: 1fr 1fr 1fr;
    gap: 0.6rem;
  }
  label,
  .caption {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    font-size: 0.75rem;
    color: var(--muted);
  }
  .wide {
    grid-column: 1 / -1;
  }
  input,
  textarea {
    font: inherit;
    font-size: 0.875rem;
    color: var(--text);
    background: var(--canvas);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    padding: 0.4rem 0.6rem;
  }
  textarea {
    width: 100%;
    box-sizing: border-box;
    resize: vertical;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    line-height: 1.4;
  }
  textarea:focus {
    outline: none;
    border-color: var(--gold);
  }
  .order {
    margin-top: 1rem;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.3rem;
  }
  .sections {
    list-style: none;
    margin: 0 0 0.75rem;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }
  .section-head {
    display: flex;
    align-items: center;
    gap: 0.2rem;
    margin-bottom: 0.3rem;
  }
  .label {
    width: 6rem;
    margin-right: auto;
  }
  .problems {
    flex: 1;
    font-size: 0.75rem;
    color: var(--warn);
  }
  .loading {
    padding: 2rem;
    text-align: center;
  }
</style>
