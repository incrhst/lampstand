<!-- A row of mutually exclusive choices: Show / Logo / Blank, library
     tabs, a wizard's decision rows. An option can be disabled with a
     reason, which shows as its tooltip. -->
<script lang="ts" generics="T extends string">
  let {
    options,
    value,
    onchange,
    label,
    stretch = false,
  }: {
    options: readonly { id: T; label: string; disabled?: string }[];
    value: T;
    onchange: (id: T) => void;
    /** Accessible name for the group. */
    label: string;
    /** Fill the available width, options sharing it equally. */
    stretch?: boolean;
  } = $props();

  // Arrow keys move between options, as in a native radio group.
  function onkeydown(e: KeyboardEvent) {
    if (e.key !== "ArrowRight" && e.key !== "ArrowLeft") return;
    const enabled = options.filter((o) => !o.disabled);
    const at = Math.max(0, enabled.findIndex((o) => o.id === value));
    const step = e.key === "ArrowRight" ? 1 : -1;
    const next = enabled[(at + step + enabled.length) % enabled.length];
    if (next) {
      e.preventDefault();
      e.stopPropagation();
      onchange(next.id);
    }
  }
</script>

<div class="group" class:stretch role="radiogroup" aria-label={label} tabindex="-1" {onkeydown}>
  {#each options as o (o.id)}
    <button
      type="button"
      role="radio"
      aria-checked={value === o.id}
      tabindex={value === o.id || (!options.some((x) => x.id === value) && o === options.find((x) => !x.disabled)) ? 0 : -1}
      disabled={!!o.disabled}
      title={o.disabled}
      onclick={() => onchange(o.id)}
    >
      {o.label}
    </button>
  {/each}
</div>

<style>
  .group {
    display: inline-flex;
    border: 1px solid var(--line);
    background: var(--canvas);
    border-radius: var(--radius);
    padding: 2px;
    gap: 2px;
  }
  .stretch {
    display: flex;
  }
  .stretch button {
    flex: 1;
  }
  button {
    font: inherit;
    font-size: 0.78rem;
    font-weight: 500;
    white-space: nowrap;
    background: none;
    border: none;
    border-radius: 4px;
    padding: 0.3rem 0.7rem;
    color: var(--muted);
    cursor: pointer;
    /* No transition: selection shows at once, and a window that is not
       frontmost can freeze a transition part way. */
  }
  button:hover:not(:disabled) {
    color: var(--text);
  }
  button[aria-checked="true"] {
    background: var(--raised);
    color: var(--text);
  }
  button:disabled {
    color: var(--faint);
    opacity: 0.6;
    cursor: not-allowed;
  }
</style>
