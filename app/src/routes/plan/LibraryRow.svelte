<!-- One library row: click (or Enter from the search) adds it to the
     service, drag it to place it in the order. -->
<script lang="ts">
  import type { Entry } from "../../lib/library";
  import { isVideo } from "../../lib/media";
  import Icon from "../../lib/kit/Icon.svelte";
  import type { IconName } from "../../lib/kit/icons";

  let {
    entry,
    id,
    active,
    added,
    inService,
    onadd,
    onhover,
    onpointerdown,
  }: {
    entry: Entry;
    id?: string;
    /** The keyboard highlight. */
    active: boolean;
    /** Just added: shows "Added" for a moment. */
    added: boolean;
    inService: boolean;
    onadd: () => void;
    onhover: () => void;
    onpointerdown: (e: PointerEvent) => void;
  } = $props();

  const icon = (e: Entry): IconName =>
    e.kind === "media" ? (isVideo(e.title) ? "media" : "announcements") : e.kind === "song" ? "song" : e.kind;
</script>

<li>
  <button
    type="button"
    {id}
    class="row"
    class:active
    class:unavailable={!entry.item && entry.key !== "link"}
    onclick={onadd}
    onmouseenter={onhover}
    {onpointerdown}
    aria-label="{entry.title}{inService ? ', in service' : ''}"
  >
    <span class="kind"><Icon name={entry.key === "link" ? "deck" : icon(entry)} /></span>
    <span class="text">
      <span class="title">{entry.title}</span>
      <span class="subtitle" class:snippet={!!entry.snippet}>{entry.snippet ?? entry.subtitle}</span>
    </span>
    {#if added}
      <span class="hint added"><Icon name="check" size={14} />Added</span>
    {:else if inService}
      <span class="hint in-service"><Icon name="check" size={14} /><span class="label">In service</span><span class="again">Add again</span></span>
    {:else if entry.item || entry.key === "link"}
      <span class="hint add"><Icon name="plus" size={14} />Add</span>
    {/if}
  </button>
</li>

<style>
  .row {
    font: inherit;
    color: inherit;
    text-align: left;
    background: none;
    border: none;
    border-radius: var(--radius);
    width: 100%;
    display: flex;
    align-items: center;
    gap: 0.7rem;
    padding: 0.5rem 0.6rem;
    cursor: pointer;
    touch-action: none;
    user-select: none;
  }
  .row:hover {
    background: color-mix(in srgb, var(--raised) 60%, transparent);
  }
  .row.active {
    background: var(--raised);
  }
  .row.unavailable {
    cursor: default;
  }
  .kind {
    width: 1.9rem;
    height: 1.9rem;
    flex-shrink: 0;
    border-radius: var(--radius);
    background: var(--raised);
    color: var(--muted);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .active .kind {
    background: var(--line);
  }
  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 0.05rem;
  }
  .title,
  .subtitle {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .title {
    font-size: 0.875rem;
    font-weight: 500;
  }
  .subtitle {
    font-size: 0.75rem;
    color: var(--muted);
  }
  .snippet {
    font-style: italic;
    color: var(--faint);
  }
  .unavailable .title {
    color: var(--muted);
  }
  .hint {
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
    flex-shrink: 0;
    font-size: 0.75rem;
    color: var(--muted);
  }
  .hint.add {
    opacity: 0;
  }
  .row:hover .add,
  .row.active .add {
    opacity: 1;
  }
  .added {
    color: var(--gold);
  }
  .in-service :global(.icon) {
    color: var(--ready-text);
  }
  .in-service .again {
    display: none;
  }
  .row:hover .in-service .label,
  .row.active .in-service .label {
    display: none;
  }
  .row:hover .in-service .again,
  .row.active .in-service .again {
    display: inline;
  }
</style>
