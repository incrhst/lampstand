<!-- The strip along the bottom of the window: connection on the left,
     background work in the middle, screens on the right. -->
<script lang="ts">
  import type { Snippet } from "svelte";

  let { start, middle, end }: { start?: Snippet; middle?: Snippet; end?: Snippet } = $props();
</script>

<footer>
  <div class="part">{#if start}{@render start()}{/if}</div>
  <div class="part middle" aria-live="polite">{#if middle}{@render middle()}{/if}</div>
  <div class="part end">{#if end}{@render end()}{/if}</div>
</footer>

<style>
  footer {
    flex-shrink: 0;
    height: 2rem;
    display: grid;
    /* The sides keep their words; a long message in the middle is cut,
       not run over them. */
    grid-template-columns: minmax(max-content, 1fr) minmax(0, auto) minmax(max-content, 1fr);
    align-items: center;
    gap: 1rem;
    padding: 0 1rem;
    border-top: 1px solid var(--line);
    background: var(--panel);
    font-size: 0.75rem;
    color: var(--muted);
  }
  .part {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    min-width: 0;
    white-space: nowrap;
  }
  .middle {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .end {
    justify-content: flex-end;
  }
</style>
