<!-- The button that folds a box to one line and opens it again (the Sound box
     in Present, the "Did you know?" card). It says whether the box is open
     (aria-expanded) and which part it shows (aria-controls). Given a key, the
     choice is remembered on this computer (lib/disclosure.ts). Anything
     inside is the button's text; with nothing inside, `label` names it. -->
<script lang="ts">
  import type { Snippet } from "svelte";
  import Icon from "./Icon.svelte";
  import { saveOpen } from "../disclosure";

  let {
    open = $bindable(),
    label,
    controls,
    key,
    children,
  }: { open: boolean; label: string; controls?: string; key?: string; children?: Snippet } = $props();

  function toggle() {
    open = !open;
    if (key) saveOpen(key, open);
  }
</script>

<button
  type="button"
  class="disclosure"
  class:folded={!open}
  aria-expanded={open}
  aria-controls={controls}
  aria-label={children ? undefined : label}
  title={open ? `Fold ${label}` : `Open ${label}`}
  onclick={toggle}
>
  <Icon name="chevronDown" size={14} />
  {#if children}{@render children()}{/if}
</button>

<style>
  .disclosure {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    min-width: 0;
    font: inherit;
    color: inherit;
    background: none;
    border: none;
    border-radius: var(--radius);
    padding: 0.1rem 0.15rem;
    cursor: pointer;
    text-align: left;
  }
  .disclosure:hover {
    background: var(--panel);
  }
  .disclosure :global(.icon) {
    color: var(--muted);
    transition: transform 120ms ease;
  }
  .folded :global(.icon) {
    transform: rotate(-90deg);
  }
</style>
