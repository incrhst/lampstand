<!-- Marks a spot whose data the core cannot give yet, naming the ticket
     that will back it, so a mock state is never mistaken for a real one.
     Remove it when that ticket lands. -->
<script lang="ts">
  import type { Snippet } from "svelte";

  let { ticket, children }: { ticket: string; children?: Snippet } = $props();
</script>

<span class="placeholder" title="Not built yet: arrives with {ticket}">
  {#if children}{@render children()}{/if}
  <span class="ticket">{ticket}</span>
</span>

<style>
  .placeholder {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    border: 1px dashed var(--line-strong);
    border-radius: 999px;
    padding: 0.15rem 0.6rem;
    font-size: 0.75rem;
    color: var(--muted);
    white-space: nowrap;
  }
  .ticket {
    font-size: 0.68rem;
    font-weight: 600;
    color: var(--faint);
    letter-spacing: 0.03em;
  }
</style>
