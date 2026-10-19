<script lang="ts">
  import type { Snippet } from "svelte";

  let {
    onclick,
    children,
    variant = "normal",
    pressed,
    disabled = false,
    title,
    shortcut,
  }: {
    onclick: () => void;
    children: Snippet;
    variant?: "normal" | "primary" | "danger";
    pressed?: boolean;
    disabled?: boolean;
    title?: string;
    /** Shown after the label, e.g. "Space". */
    shortcut?: string;
  } = $props();
</script>

<button type="button" class={variant} {onclick} {disabled} {title} aria-pressed={pressed}>
  {@render children()}
  {#if shortcut}<kbd>{shortcut}</kbd>{/if}
</button>

<style>
  button {
    font: inherit;
    padding: 0.45rem 0.9rem;
    border-radius: 6px;
    border: 1px solid var(--line);
    background: var(--raised);
    color: var(--text);
    cursor: pointer;
    display: inline-flex;
    gap: 0.5rem;
    align-items: center;
  }
  button:hover:not(:disabled) {
    border-color: var(--accent);
  }
  button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .primary {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--on-accent);
  }
  .danger[aria-pressed="true"],
  .normal[aria-pressed="true"] {
    background: var(--danger);
    border-color: var(--danger);
    color: white;
  }
  kbd {
    font-size: 0.75em;
    opacity: 0.7;
    font-family: inherit;
  }
</style>
