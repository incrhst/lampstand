<!-- One library row: click (or Enter from the search) adds it to the
     service, drag it to place it in the order. -->
<script lang="ts">
  import type { Entry } from "../../lib/library";
  import { isVideo } from "../../lib/media";
  import Icon from "../../lib/kit/Icon.svelte";
  import DeckThumb from "../../lib/kit/DeckThumb.svelte";
  import MediaThumb from "../../lib/kit/MediaThumb.svelte";
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
    onedit,
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
    /** For a song: open it in the editor. */
    onedit?: () => void;
  } = $props();

  /** A pasted link's row that fetches something; the streaming row only says why not. */
  const fetchable = (e: Entry) => e.key === "link" || e.key === "link:sound";

  const icon = (e: Entry): IconName =>
    e.key === "link" ? "deck" : e.key.startsWith("link:") ? "sound" : e.kind === "media" ? (isVideo(e.title) ? "media" : "announcements") : e.kind === "song" ? "song" : e.kind;
</script>

<li class="item">
  <button
    type="button"
    {id}
    class="row"
    class:active
    class:unavailable={!entry.item && !fetchable(entry)}
    onclick={onadd}
    onmouseenter={onhover}
    {onpointerdown}
    aria-label="{entry.title}{inService ? ', in service' : ''}"
  >
    {#if entry.item && "Media" in entry.item}
      <MediaThumb blob={entry.item.Media.blob} name={entry.item.Media.name} icon={icon(entry)} />
    {:else if entry.item && "Deck" in entry.item}
      <DeckThumb deck={entry.item.Deck.deck} version={entry.version} />
    {:else}
      <span class="kind"><Icon name={icon(entry)} /></span>
    {/if}
    <span class="text">
      <span class="title">{entry.title}{#if entry.fresh && !inService}<span class="new">New</span>{/if}</span>
      <span class="subtitle" class:snippet={!!entry.snippet}>{entry.snippet ?? entry.subtitle}</span>
    </span>
    {#if added}
      <span class="hint added"><Icon name="check" size={14} />Added</span>
    {:else if inService}
      <span class="hint in-service"><Icon name="check" size={14} /><span class="label">In service</span><span class="again">Add again</span></span>
    {:else if entry.item || fetchable(entry)}
      <span class="hint add"><Icon name="plus" size={14} />Add</span>
    {/if}
  </button>
  {#if onedit}
    <button type="button" class="edit" onclick={onedit} aria-label="Edit {entry.title}" title="Edit this song">
      <Icon name="setup" size={14} />
    </button>
  {/if}
</li>

<style>
  .item {
    display: flex;
    align-items: center;
  }
  .item .row {
    flex: 1;
    min-width: 0;
    width: auto;
  }
  .edit {
    display: inline-flex;
    flex-shrink: 0;
    padding: 0.4rem;
    margin-right: 0.3rem;
    color: var(--muted);
    background: none;
    border: none;
    border-radius: var(--radius);
    cursor: pointer;
    opacity: 0;
  }
  .item:hover .edit,
  .edit:focus-visible {
    opacity: 1;
  }
  .edit:hover {
    color: var(--text);
    background: var(--line);
  }
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
    width: var(--tile-width);
    height: var(--tile-height);
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
  .new {
    margin-left: 0.5rem;
    padding: 0 0.35rem;
    font-size: 0.65rem;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--gold);
    border: 1px solid currentColor;
    border-radius: var(--radius);
    vertical-align: 0.1em;
  }
</style>
