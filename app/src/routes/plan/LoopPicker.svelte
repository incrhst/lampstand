<!-- Loop an item or section, for pre- and post-service: seconds each slide
     stays up, blank for no loop. On a header it loops the items after it up
     to the next header; on any other item, that item's own slides. -->
<script lang="ts">
  import Icon from "../../lib/kit/Icon.svelte";

  let {
    value,
    section,
    label,
    onchange,
  }: {
    value: number | null;
    /** A header: the loop covers the section after it. */
    section: boolean;
    label: string;
    onchange: (seconds: number | null) => void;
  } = $props();

  let text = $state("");
  $effect(() => {
    text = value ? String(value) : "";
  });

  function commit() {
    const n = Math.round(Number(text));
    const seconds = text.trim() !== "" && n > 0 ? Math.min(n, 3600) : null;
    text = seconds ? String(seconds) : "";
    if (seconds !== (value ?? null)) onchange(seconds);
  }
</script>

<label class="picker" title={section ? "Loop this section: seconds per slide" : "Loop this item: seconds per slide"}>
  <Icon name="play" size={12} />
  <input
    type="number"
    min="1"
    max="3600"
    placeholder={section ? "Loop section (s)" : "Loop (s)"}
    aria-label={label}
    bind:value={text}
    onchange={commit}
    onkeydown={(e) => {
      if (e.key === "Enter") (e.currentTarget as HTMLInputElement).blur();
      if (e.key === "Escape") {
        text = value ? String(value) : "";
        (e.currentTarget as HTMLInputElement).blur();
      }
    }}
  />
</label>

<style>
  .picker {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    color: var(--faint);
    flex-shrink: 0;
  }
  .picker input {
    width: 7.5rem;
    font-size: 0.75rem;
    padding: 0.2rem 0.4rem;
  }
  .picker:not(:focus-within) input:placeholder-shown {
    border-color: transparent;
    background: transparent;
  }
</style>
