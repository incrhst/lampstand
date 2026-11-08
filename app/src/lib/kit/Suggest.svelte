<!-- A text box with suggestions under it: a font, an OBS scene. It stands in
     for <input list> and <datalist>, whose list the webview draws itself
     (on macOS, dark words on dark glass that can't be read) and that the
     app can't style. Anything can still be typed; the list only helps.
     The list is position: fixed so a scrolling pane or a row doesn't clip
     it. -->
<script lang="ts">
  import type { HTMLInputAttributes } from "svelte/elements";

  export type Suggestion = { value: string; note?: string };

  let {
    value = $bindable(""),
    options,
    /** Draw each suggestion in its own font (for a font list). */
    fontPreview = false,
    /** A suggestion was chosen from the list. */
    onpick,
    oninput,
    onkeydown,
    onfocus,
    onblur,
    ...rest
  }: {
    value?: string;
    options: readonly Suggestion[];
    fontPreview?: boolean;
    onpick?: (value: string) => void;
  } & Omit<HTMLInputAttributes, "value" | "list"> = $props();

  const id = `suggest-${Math.random().toString(36).slice(2)}`;
  let input: HTMLInputElement | undefined = $state();
  let open = $state(false);
  let active = $state(-1);
  let typed = $state(false);
  let place = $state({ left: 0, top: 0, width: 0 });

  // Like a datalist: everything until something is typed, then the matches.
  let shown = $derived.by(() => {
    const q = value.trim().toLowerCase();
    if (!typed || !q) return options;
    return options.filter((o) => o.value.toLowerCase().includes(q));
  });

  function position() {
    if (!input) return;
    const r = input.getBoundingClientRect();
    place = { left: r.left, top: r.bottom + 4, width: r.width };
  }

  function show() {
    if (!options.length) return;
    position();
    open = true;
  }

  function hide() {
    open = false;
    active = -1;
    typed = false;
  }

  function pick(o: Suggestion) {
    value = o.value;
    hide();
    onpick?.(o.value);
  }

  $effect(() => {
    if (!open) return;
    const again = () => position();
    window.addEventListener("scroll", again, true);
    window.addEventListener("resize", again);
    return () => {
      window.removeEventListener("scroll", again, true);
      window.removeEventListener("resize", again);
    };
  });
</script>

<input
  {...rest}
  bind:this={input}
  bind:value
  role="combobox"
  autocomplete="off"
  aria-autocomplete="list"
  aria-expanded={open && shown.length > 0}
  aria-controls={id}
  aria-activedescendant={open && active >= 0 ? `${id}-${active}` : undefined}
  onfocus={(e) => {
    show();
    onfocus?.(e);
  }}
  onblur={(e) => {
    hide();
    onblur?.(e);
  }}
  oninput={(e) => {
    typed = true;
    active = -1;
    show();
    oninput?.(e);
  }}
  onkeydown={(e) => {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (!open) show();
      const n = shown.length;
      if (n) active = e.key === "ArrowDown" ? (active + 1) % n : (active - 1 + n) % n;
      return;
    }
    if (e.key === "Enter" && open && active >= 0 && shown[active]) {
      e.preventDefault();
      pick(shown[active]);
      return;
    }
    if (e.key === "Escape" && open) {
      e.preventDefault();
      hide();
      return;
    }
    onkeydown?.(e);
  }}
/>

{#if open && shown.length}
  <ul
    class="list"
    {id}
    role="listbox"
    style:left="{place.left}px"
    style:top="{place.top}px"
    style:min-width="{place.width}px"
  >
    {#each shown as o, i (o.value)}
      <!-- mousedown, so the box keeps focus and doesn't close the list first. -->
      <li
        id="{id}-{i}"
        role="option"
        aria-selected={i === active}
        class:active={i === active}
        class:current={o.value === value}
        onmousedown={(e) => {
          e.preventDefault();
          pick(o);
        }}
        onmousemove={() => (active = i)}
      >
        <span class="value" style:font-family={fontPreview ? `"${o.value}", var(--font, system-ui)` : undefined}>{o.value}</span>
        {#if o.note}<span class="note">{o.note}</span>{/if}
      </li>
    {/each}
  </ul>
{/if}

<style>
  .list {
    position: fixed;
    z-index: 1000;
    max-height: 16rem;
    overflow-y: auto;
    margin: 0;
    padding: 0.3rem;
    list-style: none;
    background: var(--raised);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.45);
  }
  li {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 1rem;
    padding: 0.45rem 0.6rem;
    border-radius: calc(var(--radius) - 2px);
    color: var(--text);
    font-size: 0.9rem;
    cursor: pointer;
  }
  li.active {
    background: var(--gold-soft);
  }
  li.current .value {
    color: var(--gold);
  }
  .note {
    color: var(--muted);
    font-size: 0.8rem;
    white-space: nowrap;
  }
</style>
