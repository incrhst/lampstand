<!-- Present's quiet corner: the controls that are not used every minute of a
     service (safe area, clicker mode, clearing) and the keys, so
     the bar keeps only what is run live. Escape or a click outside closes it. -->
<script lang="ts">
  import Button from "../../lib/kit/Button.svelte";
  import Icon from "../../lib/kit/Icon.svelte";
  import { KEYS as keys } from "../../lib/shortcuts";
  import type { IconName } from "../../lib/kit/icons";

  let {
    guide,
    clicker,
    bug,
    bugReady,
    obs,
    mod,
    onguide,
    onclicker,
    onbug,
    onclearText,
    onclearAll,
    onidle,
  }: {
    /** The safe-area guide is drawn on the preview. */
    guide: boolean;
    clicker: boolean;
    /** The logo bug is on. */
    bug: boolean;
    /** The church has a logo to put in the corner. */
    bugReady: boolean;
    /** The service has an OBS control, so the O key applies. */
    obs: boolean;
    /** "⌘" or "Ctrl+", for the screen keys. */
    mod: string;
    onguide: () => void;
    onclicker: () => void;
    onbug: () => void;
    onclearText: () => void;
    onclearAll: () => void;
    /** Opens where the idle screen is chosen. */
    onidle?: () => void;
  } = $props();

  let open = $state(false);
  let root: HTMLSpanElement | undefined = $state();

  /** Runs an action and closes the menu. */
  const then = (action: () => void) => () => {
    open = false;
    action();
  };

  function onwindowclick(e: MouseEvent) {
    if (open && root && !root.contains(e.target as Node)) open = false;
  }
  function onwindowkey(e: KeyboardEvent) {
    if (open && e.key === "Escape") {
      e.preventDefault();
      open = false;
    }
  }

</script>

<svelte:window onclick={onwindowclick} onkeydown={onwindowkey} />

<span class="more" bind:this={root}>
  <Button size="small" pressed={open} onclick={() => (open = !open)} title="Safe area, clicker, logo bug, clearing and the keys">More</Button>
  {#if open}
    <div class="menu" role="menu" aria-label="More controls">
      {@render item("safeArea", "Safe area guide", guide ? "On" : "Off", then(onguide), guide)}
      {@render item("keyboard", "Clicker mode", clicker ? "On" : "Off", then(onclicker), clicker)}
      {@render item("image", "Logo bug", bugReady ? (bug ? "On" : "Off") : "Add a logo in Setup", then(onbug), bugReady ? bug : undefined, !bugReady)}
      {#if onidle}{@render item("image", "Change idle screen", "What shows when nothing is on air", then(onidle))}{/if}
      {@render item("x", "Clear text", "Keep the background", then(onclearText))}
      {@render item("x", "Clear all", "Text, background and overlay", then(onclearAll))}
      <h3>Keys</h3>
      <dl>
        {#each keys as [k, what]}
          <dt>{k}</dt>
          <dd>{what}</dd>
        {/each}
        {#if obs}
          <dt>O</dt>
          <dd>Control OBS</dd>
        {/if}
        <dt>{mod}1 · {mod}2 · {mod}\</dt>
        <dd>Plan · Present · switch between them</dd>
      </dl>
    </div>
  {/if}
</span>

{#snippet item(icon: IconName, label: string, hint: string, run: () => void, pressed?: boolean, disabled = false)}
  <button type="button" class="row" role={pressed === undefined ? "menuitem" : "menuitemcheckbox"} aria-checked={pressed} {disabled} onclick={run}>
    <Icon name={icon} size={14} />
    <span class="label">{label}</span>
    <span class="hint" class:on={pressed}>{hint}</span>
  </button>
{/snippet}

<style>
  .more {
    position: relative;
    display: inline-flex;
  }
  .menu {
    position: absolute;
    top: calc(100% + 0.4rem);
    right: 0;
    z-index: 20;
    width: 22rem;
    padding: 0.35rem;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-lg);
    background: var(--panel);
    box-shadow: 0 12px 32px rgba(0, 0, 0, 0.45);
  }
  .row {
    font: inherit;
    font-size: 0.85rem;
    color: var(--text);
    width: 100%;
    display: flex;
    align-items: center;
    gap: 0.6rem;
    padding: 0.5rem 0.6rem;
    border: none;
    border-radius: var(--radius);
    background: none;
    text-align: left;
    cursor: pointer;
  }
  .row:disabled {
    cursor: default;
    opacity: 0.6;
  }
  .row:hover:not(:disabled) {
    background: var(--raised);
  }
  .label {
    flex-shrink: 0;
  }
  .hint {
    margin-left: auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.75rem;
    color: var(--muted);
  }
  .hint.on {
    color: var(--gold);
  }
  h3 {
    margin: 0.5rem 0.6rem 0.25rem;
    padding-top: 0.6rem;
    border-top: 1px solid var(--line);
    font-size: 0.7rem;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--faint);
  }
  dl {
    margin: 0 0.6rem 0.4rem;
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 0.3rem 0.9rem;
    font-size: 0.78rem;
  }
  dt {
    color: var(--text);
    white-space: nowrap;
  }
  dd {
    margin: 0;
    color: var(--muted);
  }
</style>
