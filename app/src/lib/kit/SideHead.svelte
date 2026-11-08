<!-- The head of the Library & controls sidebar, the same in Plan and
     Present. On its front page it names the sidebar; on a page inside it
     (the Library, Services) it shows Back and that page's name. Hide is
     always here, with its key, so the way out is never only a shortcut. -->
<script lang="ts">
  import Icon from "./Icon.svelte";

  let {
    title = "Library & controls",
    onback,
    onhide,
  }: { title?: string; onback?: () => void; onhide: () => void } = $props();
</script>

<div class="side-head">
  {#if onback}
    <button type="button" class="back" onclick={onback} title="Back to Library & controls (Esc)">
      <Icon name="prev" size={14} />Back
    </button>
    <span class="slash" aria-hidden="true">/</span>
    <h2 class="crumb">{title}</h2>
  {:else}
    <h2 class="name">{title}</h2>
  {/if}
  <button type="button" class="hide" onclick={onhide} title="Hide the Library and the controls (F)">Hide <kbd>F</kbd></button>
</div>

<style>
  .side-head {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    min-height: 2.25rem;
  }
  h2 {
    margin: 0;
    font-size: 0.875rem;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .name {
    font-weight: 500;
    color: var(--muted);
  }
  .crumb {
    font-weight: 600;
    color: var(--text);
  }
  .slash {
    color: var(--faint);
  }
  button {
    font: inherit;
    font-size: 0.8125rem;
    color: var(--muted);
    background: none;
    border: none;
    border-radius: 4px;
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0.25rem 0.4rem;
    white-space: nowrap;
  }
  button:hover {
    color: var(--text);
    background: var(--raised);
  }
  .back {
    margin-left: -0.4rem;
  }
  .hide {
    margin-left: auto;
    color: var(--faint);
  }
  kbd {
    font: inherit;
    font-size: 0.72em;
    opacity: 0.8;
    padding: 0 0.3rem;
    border: 1px solid currentColor;
    border-radius: 3px;
  }
</style>
