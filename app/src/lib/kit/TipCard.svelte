<!-- A dismissible "Did you know?" card for one tip, with a way to the rest.
     It folds to one line ("Did you know?" and the tip's title) and opens
     again with a click or Enter; folded or open is remembered on this
     computer. Folding is not dismissing: the tip stays, and its buttons work
     as before once it is open. -->
<script lang="ts">
  import Button from "./Button.svelte";
  import Disclosure from "./Disclosure.svelte";
  import Icon from "./Icon.svelte";
  import type { Tip } from "../tips";
  import { TIP_KEY, openState } from "../disclosure";

  // A tip with an action also has "Not now" (put off) and "Don't ask again" (dismiss for good).
  let {
    tip,
    ondismiss,
    onall,
    onaction,
    onsnooze,
  }: { tip: Tip; ondismiss: () => void; onall: () => void; onaction?: () => void; onsnooze?: () => void } = $props();
  let acts = $derived(!!tip.action && !!onaction);
  let open = $state(openState(TIP_KEY, true));
  const id = $props.id();
</script>

<aside class="tip" class:folded={!open} aria-label="Did you know?">
  <Disclosure bind:open label="Did you know?" controls={id} key={TIP_KEY}>
    <span class="eyebrow"><Icon name="lamp" size={14} />Did you know?</span>
    {#if !open}<strong class="line">{tip.title}</strong>{/if}
  </Disclosure>
  {#if open}
    <div class="open" {id}>
      <strong>{tip.title}</strong>
      <p>{tip.body}</p>
      <div class="actions">
        {#if acts && tip.action}
          <Button size="small" variant="primary" onclick={() => onaction?.()}>{tip.action.label}</Button>
          {#if tip.snoozeDays && onsnooze}<Button size="small" onclick={() => onsnooze?.()}>Not now</Button>{/if}
          <button type="button" class="link" onclick={ondismiss}>Don't ask again</button>
        {:else}
          <Button size="small" onclick={ondismiss}>Got it</Button>
          <button type="button" class="link" onclick={onall}>All quick tips</button>
        {/if}
      </div>
    </div>
  {/if}
</aside>

<style>
  .tip {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    margin: 0 0.75rem 0.5rem;
    padding: 0.75rem;
    border: 1px dashed var(--line-strong);
    border-radius: var(--radius-lg);
    font-size: 0.8125rem;
  }
  .tip.folded {
    padding: 0.4rem 0.6rem;
  }
  .open {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
  }
  .line {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .eyebrow {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    font-size: 0.7rem;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--muted);
  }
  p {
    margin: 0;
    color: var(--muted);
    line-height: 1.4;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    margin-top: 0.25rem;
    flex-wrap: wrap;
  }
  .link {
    font: inherit;
    background: none;
    border: none;
    padding: 0;
    color: var(--text);
    text-decoration: underline;
    cursor: pointer;
  }
</style>
