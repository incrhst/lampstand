<!-- A small picture that is asked for only when it scrolls into view, with
     the kind's icon until it arrives and when there is none (not on this
     device yet, or it cannot be drawn). `load` answers a `data:` URL or
     null; the callers remember answers, so a row drawn again is not asked
     again. -->
<script lang="ts">
  import Icon from "./Icon.svelte";
  import type { IconName } from "./icons";

  let { icon, load }: { icon: IconName; load: () => Promise<string | null> } = $props();

  let src: string | null = $state(null);
  let root: HTMLSpanElement | undefined = $state();
  let wanted = $state(false);

  $effect(() => {
    if (!root || wanted) return;
    if (typeof IntersectionObserver === "undefined") {
      wanted = true;
      return;
    }
    const seen = new IntersectionObserver((entries) => {
      if (entries.some((e) => e.isIntersecting)) {
        wanted = true;
        seen.disconnect();
      }
    });
    seen.observe(root);
    return () => seen.disconnect();
  });

  $effect(() => {
    if (!wanted) return;
    let current = true;
    load().then((url) => {
      if (current) src = url;
    });
    return () => {
      current = false;
    };
  });
</script>

<span class="thumb" bind:this={root}>
  {#if src}
    <img {src} alt="" loading="lazy" />
  {:else}
    <Icon name={icon} />
  {/if}
</span>

<style>
  .thumb {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: var(--tile-width);
    height: var(--tile-height);
    overflow: hidden;
    flex-shrink: 0;
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--panel) 70%, black);
    color: var(--muted);
  }
  img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }
</style>
