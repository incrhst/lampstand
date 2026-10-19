<script lang="ts">
  import type { Snippet } from "svelte";

  let { title, children, actions }: { title: string; children: Snippet; actions?: Snippet } =
    $props();
</script>

<section>
  <header>
    <h2>{title}</h2>
    {#if actions}<div class="actions">{@render actions()}</div>{/if}
  </header>
  {@render children()}
</section>

<style>
  section {
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: 8px;
    padding: 0.8rem 1rem 1rem;
    min-width: 0;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    margin-bottom: 0.6rem;
  }
  h2 {
    font-size: 0.95rem;
    margin: 0;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
  }
  .actions {
    display: flex;
    gap: 0.5rem;
  }
</style>
