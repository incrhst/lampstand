<!-- A titled card, for settings-like screens (Outputs). Working screens
     use full-height columns (.pane in theme.css) instead. -->
<script lang="ts">
  import type { Snippet } from "svelte";

  let {
    title,
    description,
    children,
    actions,
  }: { title: string; description?: string; children: Snippet; actions?: Snippet } = $props();
</script>

<section>
  <header>
    <div>
      <h2>{title}</h2>
      {#if description}<p class="muted">{description}</p>{/if}
    </div>
    {#if actions}<div class="actions">{@render actions()}</div>{/if}
  </header>
  {@render children()}
</section>

<style>
  section {
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    padding: 1rem 1.25rem 1.25rem;
    min-width: 0;
  }
  header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 1rem;
    margin-bottom: 0.75rem;
  }
  h2 {
    font-size: 1rem;
    font-weight: 600;
  }
  p {
    font-size: 0.85rem;
    margin-top: 0.2rem;
  }
  .actions {
    display: flex;
    gap: 0.5rem;
  }
</style>
