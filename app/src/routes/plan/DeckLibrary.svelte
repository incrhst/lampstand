<!-- The library's Decks tab: the decks on this device, a field for a
     pasted share link, and a button to add a .pptx from disk. Adding a
     deck here puts it in the library and in the selected service. -->
<script lang="ts">
  import type { Deck } from "../../lib/api";
  import { deckSourceLabel } from "../../lib/media";
  import Button from "../../lib/kit/Button.svelte";
  import Icon from "../../lib/kit/Icon.svelte";
  import ListRow from "../../lib/kit/ListRow.svelte";

  let {
    decks,
    inService,
    canAdd,
    busy,
    onadd,
    onpick,
    onlink,
  }: {
    decks: Deck[];
    /** Deck IDs already in the selected service. */
    inService: Set<string>;
    /** A service is selected. */
    canAdd: boolean;
    busy: boolean;
    onadd: (deck: string) => void;
    onpick: () => void;
    /** Resolves true once the link's deck is added, so the field clears. */
    onlink: (url: string) => Promise<boolean>;
  } = $props();

  let link = $state("");

  async function addLink() {
    if (!link.trim()) return;
    if (await onlink(link.trim())) link = "";
  }
</script>

<form
  class="link"
  onsubmit={(e) => {
    e.preventDefault();
    addLink();
  }}
>
  <input
    type="url"
    aria-label="Share link to a deck"
    placeholder="Paste a Google Slides or OneDrive link"
    bind:value={link}
  />
  <Button size="small" onclick={addLink} disabled={busy || !link.trim()}>Add link</Button>
</form>

<ul class="plain pane-body">
  {#each decks as deck (deck.id)}
    <ListRow title={deck.title} subtitle={deckSourceLabel(deck.source)}>
      {#snippet leading()}<Icon name="deck" />{/snippet}
      {#snippet trailing()}
        {#if inService.has(deck.id)}
          <span class="in-service"><Icon name="check" size={14} />In service</span>
        {:else}
          <Button size="small" icon="plus" onclick={() => onadd(deck.id)} disabled={!canAdd} label="Add {deck.title} to service">
            Add
          </Button>
        {/if}
      {/snippet}
    </ListRow>
  {:else}
    <li class="empty muted">
      <p>No decks yet.</p>
      <p>Add a PowerPoint file, or paste a link to one shared as “anyone with the link”.</p>
    </li>
  {/each}
</ul>

<div class="foot">
  <Button size="small" icon="upload" onclick={onpick} disabled={busy}>Add PowerPoint file…</Button>
</div>

<style>
  .link {
    display: flex;
    gap: 0.5rem;
    padding: 0 1rem 0.75rem;
  }
  .link input {
    flex: 1;
    min-width: 0;
    padding: 0.3rem 0.5rem;
    font-size: 0.85rem;
  }
  .empty {
    padding: 2rem 0.75rem;
    text-align: center;
    font-size: 0.875rem;
  }
  .empty p + p {
    margin-top: 0.25rem;
    font-size: 0.8rem;
  }
  .in-service {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    height: 1.75rem;
    padding: 0 0.4rem;
  }
  .in-service :global(.icon) {
    color: var(--ready-text);
  }
  .foot {
    border-top: 1px solid var(--line);
    padding: 0.75rem 1rem 1rem;
    display: flex;
    gap: 0.5rem;
  }
</style>
