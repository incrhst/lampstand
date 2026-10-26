<!-- A card in Present's sidecard: a quiet heading over plain rows. Given a
     key it folds to its heading, and whether it is open is remembered on
     this computer (lib/disclosure.ts); folded, `summary` says what matters
     in it (a switch that is on, a remote connected). -->
<script lang="ts">
  import type { Snippet } from "svelte";
  import Disclosure from "./Disclosure.svelte";
  import { openState } from "../disclosure";

  let {
    title,
    key,
    open: initial = true,
    summary,
    children,
  }: { title: string; key?: string; open?: boolean; summary?: string; children: Snippet } = $props();

  const id = `side-${Math.random().toString(36).slice(2, 8)}`;
  // svelte-ignore state_referenced_locally
  let open = $state(key ? openState(key, initial) : true);
</script>

<section class="side-card" class:folded={!open} aria-label={title}>
  {#if key}
    <h2 class="head">
      <Disclosure bind:open label={title} controls={id} {key}>
        <span class="title">{title}</span>
        {#if !open && summary}<span class="summary">{summary}</span>{/if}
      </Disclosure>
    </h2>
  {:else}
    <h2 class="head plain">{title}</h2>
  {/if}
  <div class="body" {id} hidden={!open}>
    {@render children()}
  </div>
</section>

<style>
  .side-card {
    background: var(--panel);
    border-radius: var(--radius-lg);
    padding: 0.6rem 0.85rem 0.8rem;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    min-width: 0;
  }
  .side-card.folded {
    padding-bottom: 0.6rem;
  }
  .head {
    font-size: 0.8125rem;
    font-weight: 600;
    color: var(--muted);
  }
  .head :global(.disclosure) {
    width: 100%;
    min-height: 2rem;
    color: inherit;
  }
  /* The chevron at the end of the heading, as in the sidecard's design. */
  .head :global(.disclosure .icon) {
    order: 2;
  }
  .head.plain {
    min-height: 2rem;
    display: flex;
    align-items: center;
  }
  .title {
    flex: 1 1 auto;
  }
  .summary {
    font-weight: 400;
    color: var(--faint);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    min-width: 0;
  }
  .body[hidden] {
    display: none;
  }
</style>
