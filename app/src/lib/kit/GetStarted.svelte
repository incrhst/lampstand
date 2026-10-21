<!-- The Get started strip: the steps from a new install to a first service,
     each one ticked off as it is done, across the top of the app until it
     is finished or hidden. Each step is a button that goes where the step
     is done. -->
<script lang="ts">
  import type { Step, StepId } from "../getStarted";
  import Button from "./Button.svelte";
  import Icon from "./Icon.svelte";

  let { steps, onstep, onhide }: { steps: Step[]; onstep: (id: StepId) => void; onhide: () => void } = $props();
</script>

<section class="strip" aria-label="Get started">
  <strong class="label">Get started</strong>
  <ol>
    {#each steps as step, i (step.id)}
      <li>
        <button type="button" class:done={step.done} onclick={() => onstep(step.id)} title={step.hint}>
          {#if step.done}
            <Icon name="checkCircle" size={16} label="Done" />
          {:else}
            <span class="num" aria-hidden="true">{i + 1}</span>
          {/if}
          <span>{step.title}</span>
        </button>
      </li>
    {/each}
  </ol>
  <Button variant="ghost" size="small" icon="x" label="Hide Get started" onclick={onhide} />
</section>

<style>
  .strip {
    display: flex;
    align-items: center;
    gap: 1rem;
    padding: 0.5rem 1rem;
    border-bottom: 1px solid var(--line);
    background: color-mix(in srgb, var(--gold) 8%, var(--panel));
    font-size: 0.85rem;
  }
  .label {
    color: var(--gold);
    font-weight: 600;
    white-space: nowrap;
  }
  ol {
    flex: 1;
    display: flex;
    flex-wrap: wrap;
    gap: 0.25rem 0.5rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  button {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    font: inherit;
    color: var(--text);
    background: transparent;
    border: 1px solid var(--line);
    border-radius: 999px;
    padding: 0.2rem 0.7rem 0.2rem 0.4rem;
    cursor: pointer;
  }
  button:hover {
    background: var(--hover, rgb(255 255 255 / 0.07));
  }
  button.done {
    color: var(--muted);
    border-color: transparent;
  }
  button.done :global(svg) {
    color: var(--ready, #6fbf73);
  }
  .num {
    display: inline-grid;
    place-items: center;
    width: 1.1rem;
    height: 1.1rem;
    border-radius: 50%;
    border: 1px solid var(--gold);
    color: var(--gold);
    font-size: 0.7rem;
  }
</style>
