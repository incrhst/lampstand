<!-- One row of a list (<ul class="plain">): a services list entry, a
     service item, a library song. The title area is a button when the
     row has onclick; trailing actions sit beside it, never inside it. -->
<script lang="ts">
  import type { Snippet } from "svelte";

  let {
    title,
    subtitle,
    onclick,
    active = false,
    accent = false,
    dim = false,
    leading,
    details,
    trailing,
    divided = false,
  }: {
    title: string;
    subtitle?: string;
    onclick?: () => void;
    /** The selected row (raised background, aria-current). */
    active?: boolean;
    /** The current row in a running order (gold tint and bar). */
    accent?: boolean;
    /** Past rows in a running order. */
    dim?: boolean;
    leading?: Snippet;
    /** Extra lines under the subtitle. */
    details?: Snippet;
    trailing?: Snippet;
    /** A rule under the row instead of rounded hover. */
    divided?: boolean;
  } = $props();
</script>

{#snippet body()}
  {#if leading}<span class="leading">{@render leading()}</span>{/if}
  <span class="text">
    <span class="title" class:dim>{title}</span>
    {#if subtitle}<span class="subtitle">{subtitle}</span>{/if}
    {#if details}{@render details()}{/if}
  </span>
{/snippet}

<li class="row" class:active class:accent class:divided class:clickable={!!onclick}>
  {#if onclick}
    <button type="button" class="main" {onclick} aria-current={active || accent ? "true" : undefined}>
      {@render body()}
    </button>
  {:else}
    <div class="main">{@render body()}</div>
  {/if}
  {#if trailing}<span class="trailing">{@render trailing()}</span>{/if}
</li>

<style>
  .row {
    position: relative;
    display: flex;
    align-items: center;
    gap: 0.5rem;
    border-radius: var(--radius);
    padding-right: 0.5rem;
    /* No transition: selection shows at once, and a window that is not
       frontmost can freeze a transition part way. */
  }
  .row.divided {
    border-radius: 0;
    border-bottom: 1px solid var(--line);
  }
  .row.divided:last-child {
    border-bottom: none;
  }
  .clickable:hover {
    background: color-mix(in srgb, var(--raised) 60%, transparent);
  }
  .active,
  .active:hover {
    background: var(--raised);
  }
  .accent,
  .accent:hover {
    background: var(--gold-soft);
  }
  .accent::before {
    content: "";
    position: absolute;
    left: 0;
    top: 0.5rem;
    bottom: 0.5rem;
    width: 2px;
    border-radius: 2px;
    background: var(--gold);
  }
  .main {
    font: inherit;
    color: inherit;
    text-align: left;
    background: none;
    border: none;
    border-radius: var(--radius);
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding: 0.6rem 0.25rem 0.6rem 0.75rem;
  }
  button.main {
    cursor: pointer;
  }
  .leading {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    color: var(--muted);
  }
  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
  }
  .title {
    font-size: 0.875rem;
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .title.dim {
    color: var(--faint);
    font-weight: 400;
  }
  .subtitle {
    font-size: 0.75rem;
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .trailing {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    flex-shrink: 0;
    font-size: 0.75rem;
    color: var(--muted);
  }
</style>
