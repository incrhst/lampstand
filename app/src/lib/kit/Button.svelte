<script lang="ts">
  import type { Snippet } from "svelte";
  import Icon from "./Icon.svelte";
  import type { IconName } from "./icons";

  let {
    onclick,
    children,
    variant = "secondary",
    size = "normal",
    icon,
    pressed,
    disabled = false,
    title,
    label,
    shortcut,
    describedby,
  }: {
    onclick: () => void;
    children?: Snippet;
    /** Primary is gold, for the one main action on a screen. Danger turns red when pressed. */
    variant?: "primary" | "secondary" | "ghost" | "danger";
    size?: "normal" | "small";
    icon?: IconName;
    pressed?: boolean;
    disabled?: boolean;
    title?: string;
    /** Accessible name, needed when there is no visible text. */
    label?: string;
    /** Shown after the label, e.g. "Space". */
    shortcut?: string;
    /** The id of text that explains the button, e.g. why it is greyed out. */
    describedby?: string;
  } = $props();
</script>

<button
  type="button"
  class="{variant} {size}"
  class:icon-only={!children}
  {onclick}
  {disabled}
  {title}
  aria-describedby={describedby}
  aria-label={label}
  aria-pressed={pressed}
>
  {#if icon}<Icon name={icon} size={size === "small" ? 14 : 16} />{/if}
  {#if children}{@render children()}{/if}
  {#if shortcut}<kbd>{shortcut}</kbd>{/if}
</button>

<style>
  button {
    font: inherit;
    font-size: 0.875rem;
    font-weight: 500;
    height: 2.25rem;
    padding: 0 0.9rem;
    border-radius: var(--radius);
    border: 1px solid transparent;
    cursor: pointer;
    display: inline-flex;
    gap: 0.5rem;
    align-items: center;
    justify-content: center;
    white-space: nowrap;
    /* No transition: a pressed state shows at once, and a window that is
       not frontmost can freeze a transition part way. */
  }
  .small {
    height: 1.75rem;
    padding: 0 0.6rem;
    font-size: 0.78rem;
    gap: 0.35rem;
  }
  .icon-only {
    width: 2.25rem;
    padding: 0;
  }
  .small.icon-only {
    width: 1.75rem;
  }
  button:disabled {
    opacity: 0.6;
    /* Grey as well as faint, so a button that cannot be used (a planning
       device's Open for live, say) does not still look gold. */
    filter: saturate(0);
    cursor: not-allowed;
  }
  .primary {
    background: var(--gold);
    color: var(--on-gold);
    font-weight: 600;
  }
  .primary:hover:not(:disabled) {
    background: var(--gold-hover);
  }
  .secondary,
  .danger {
    background: var(--raised);
    border-color: var(--line);
    color: var(--text);
  }
  .secondary:hover:not(:disabled),
  .danger:hover:not(:disabled) {
    border-color: var(--line-strong);
    background: var(--line);
  }
  .ghost {
    background: none;
    color: var(--muted);
  }
  .ghost:hover:not(:disabled) {
    background: var(--raised);
    color: var(--text);
  }
  /* Pressed beats hover, so a pressed Blank stays red under the pointer. */
  .secondary[aria-pressed="true"],
  .secondary[aria-pressed="true"]:hover:not(:disabled) {
    border-color: color-mix(in srgb, var(--gold) 40%, transparent);
    background: var(--gold-soft);
    color: var(--gold);
  }
  .danger[aria-pressed="true"],
  .danger[aria-pressed="true"]:hover:not(:disabled) {
    background: var(--live-strong);
    border-color: var(--live-strong);
    color: white;
  }
  kbd {
    font: inherit;
    font-size: 0.72em;
    opacity: 0.7;
    padding: 0 0.3rem;
    border: 1px solid currentColor;
    border-radius: 3px;
  }
</style>
