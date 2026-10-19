<!-- A wizard's numbered steps down the left: done steps tick, the
     current one is gold. For the setup wizard (T55). -->
<script lang="ts">
  import type { Snippet } from "svelte";
  import Icon from "./Icon.svelte";

  let {
    steps,
    current,
    note,
  }: {
    steps: readonly string[];
    /** Index of the step in progress. */
    current: number;
    /** Reassurance under the steps. */
    note?: Snippet;
  } = $props();
</script>

<nav class="rail" aria-label="Steps">
  <ol class="plain">
    {#each steps as step, i}
      <li aria-current={i === current ? "step" : undefined}>
        <span class="mark" class:done={i < current} class:current={i === current}>
          {#if i < current}<Icon name="check" size={14} label="Done" />{:else}{i + 1}{/if}
        </span>
        <span class="label" class:done={i < current} class:current={i === current}>{step}</span>
      </li>
    {/each}
  </ol>
  {#if note}<p class="note">{@render note()}</p>{/if}
</nav>

<style>
  .rail {
    padding: 2rem 1.5rem;
  }
  li {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding: 0.5rem 0;
  }
  .mark {
    width: 1.5rem;
    height: 1.5rem;
    border-radius: 50%;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    font-size: 0.75rem;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    border: 1px solid var(--line);
    color: var(--faint);
    flex-shrink: 0;
  }
  .mark.done {
    border-color: transparent;
    background: var(--ready-soft);
    color: var(--ready-text);
  }
  .mark.current {
    border-color: transparent;
    background: var(--gold);
    color: var(--on-gold);
  }
  .label {
    font-size: 0.875rem;
    color: var(--faint);
  }
  .label.done {
    color: var(--muted);
  }
  .label.current {
    color: var(--text);
    font-weight: 500;
  }
  .note {
    margin-top: 2rem;
    font-size: 0.78rem;
    line-height: 1.6;
    color: var(--faint);
  }
</style>
