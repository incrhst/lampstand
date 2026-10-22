<!-- The order a song is sung in, OpenSong style: "v1 v1 c v2 v1" shows those
     sections in that order and leaves the rest out. The field shows this
     service's order, or the song's own when the service has none; changing
     it sets this service's order, and blank or Reset goes back to the song's.
     "Save to song" makes the order the song's own. Labels are separated by
     spaces or commas, and case doesn't matter. A label that matches no
     section is flagged here, since the slides skip it on the day. -->
<script lang="ts">
  import Button from "../../lib/kit/Button.svelte";
  import { parseOrder, unknownLabels } from "../../lib/sections";

  let {
    sections,
    songOrder,
    value,
    label,
    onchange,
    onsave,
  }: {
    /** The song's section labels, in written order. */
    sections: string[];
    /** The song's own order; empty means the written order. */
    songOrder: string[];
    /** This service's order, or null to follow the song. */
    value: string[] | null;
    label: string;
    onchange: (order: string[] | null) => void;
    onsave: (order: string[]) => void;
  } = $props();

  let shown = $derived(value ?? (songOrder.length > 0 ? songOrder : sections));
  let text = $state("");
  $effect(() => {
    text = shown.join(" ");
  });

  let typed = $derived(parseOrder(text));
  let unknown = $derived(unknownLabels(typed, sections));
  let message = $derived(
    typed.length > 0 && unknown.length === typed.length
      ? "No section matches, so the song is shown as written"
      : unknown.length > 0
        ? `No section ${unknown.join(", ")}: skipped`
        : "",
  );

  function commit() {
    const order = parseOrder(text);
    if (order.join(" ") === shown.join(" ")) return;
    const own = songOrder.length > 0 ? songOrder : sections;
    onchange(order.length === 0 || order.join(" ") === own.join(" ") ? null : order);
  }
</script>

<span class="order" class:unknown={message !== ""}>
  <input
    type="text"
    aria-label={label}
    title="Sections in the order they are sung, for example v1 v1 c v2 v1. Sections left out are not shown."
    placeholder="Order"
    spellcheck="false"
    bind:value={text}
    onchange={commit}
    onkeydown={(e) => {
      if (e.key === "Enter") (e.currentTarget as HTMLInputElement).blur();
      if (e.key === "Escape") {
        text = shown.join(" ");
        (e.currentTarget as HTMLInputElement).blur();
      }
    }}
  />
  {#if value}
    <Button variant="ghost" size="small" onclick={() => onchange(null)} title="Use the song's own order">Reset</Button>
    <Button variant="ghost" size="small" onclick={() => onsave(value)} title="Make this the song's order everywhere">Save to song</Button>
  {/if}
  {#if message}<span class="note" role="status">{message}</span>{/if}
</span>

<style>
  .order {
    display: inline-flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.25rem;
    flex-shrink: 0;
    max-width: 18rem;
  }
  input {
    width: 9rem;
    font-size: 0.75rem;
    padding: 0.2rem 0.4rem;
  }
  .unknown input {
    border-color: color-mix(in srgb, var(--warn) 50%, transparent);
  }
  .note {
    flex-basis: 100%;
    font-size: 0.7rem;
    color: var(--warn);
  }
</style>
