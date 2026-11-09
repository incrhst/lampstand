<!-- The OBS scene an item switches to when it goes live (T90). Free text
     with OBS's scenes as suggestions, so it can be planned while OBS is
     off (the list is the last one OBS reported). A name OBS doesn't have
     is marked, since it won't switch on the day. -->
<script lang="ts">
  import Icon from "../../lib/kit/Icon.svelte";
  import Suggest from "../../lib/kit/Suggest.svelte";

  let {
    value,
    scenes,
    label,
    onchange,
  }: {
    value: string | null;
    /** OBS's scenes, top of its list first; empty if never connected. */
    scenes: string[];
    label: string;
    onchange: (scene: string | null) => void;
  } = $props();

  let text = $state("");
  $effect(() => {
    text = value ?? "";
  });

  let unknown = $derived(scenes.length > 0 && text.trim() !== "" && !scenes.includes(text.trim()));

  function commit() {
    const scene = text.trim() || null;
    if (scene !== (value ?? null)) onchange(scene);
  }
</script>

<label class="picker" class:unknown title={unknown ? "OBS has no scene with this name" : "OBS scene when this item goes live"}>
  <Icon name="play" size={12} />
  <Suggest
    type="text"
    options={scenes.map((s) => ({ value: s }))}
    onpick={commit}
    placeholder="OBS scene"
    aria-label={label}
    bind:value={text}
    onchange={commit}
    onkeydown={(e) => {
      if (e.key === "Enter") (e.currentTarget as HTMLInputElement).blur();
      if (e.key === "Escape") {
        text = value ?? "";
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
  .picker :global(input) {
    width: 9rem;
    font-size: 0.75rem;
    padding: 0.2rem 0.4rem;
  }
  .picker:not(:focus-within) :global(input:placeholder-shown) {
    border-color: transparent;
    background: transparent;
  }
  .unknown {
    color: var(--warn);
  }
  .unknown :global(input) {
    border-color: color-mix(in srgb, var(--warn) 50%, transparent);
  }
</style>
