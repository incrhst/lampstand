<!-- A row on the front page of Library & controls: an icon, a plain name,
     what is true now (summary) and a chevron. It opens a page inside the
     sidebar, or a part of Setup. -->
<script lang="ts">
  import type { Snippet } from "svelte";
  import Icon from "./Icon.svelte";
  import type { IconName } from "./icons";

  let {
    icon,
    label,
    summary,
    title,
    onclick,
  }: { icon: IconName; label: string; summary?: Snippet | string; title?: string; onclick: () => void } = $props();
</script>

<button type="button" class="side-row-nav" {onclick} {title}>
  <span class="icon"><Icon name={icon} size={17} /></span>
  <span class="label">{label}</span>
  <span class="summary">
    {#if typeof summary === "string"}{summary}{:else if summary}{@render summary()}{/if}
    <Icon name="next" size={13} />
  </span>
</button>

<style>
  .side-row-nav {
    font: inherit;
    color: var(--text);
    background: none;
    border: none;
    width: 100%;
    display: flex;
    align-items: center;
    gap: 0.7rem;
    padding: 0.5rem 0.5rem;
    border-radius: 8px;
    text-align: left;
    cursor: pointer;
    font-size: 0.9rem;
    min-width: 0;
  }
  .side-row-nav:hover {
    background: var(--raised);
  }
  .icon {
    display: inline-flex;
    color: var(--muted);
    flex-shrink: 0;
  }
  .label {
    white-space: nowrap;
  }
  .summary {
    margin-left: auto;
    min-width: 0;
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    color: var(--faint);
    font-size: 0.8rem;
    white-space: nowrap;
    overflow: hidden;
  }
</style>
