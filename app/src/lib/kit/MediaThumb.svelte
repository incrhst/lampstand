<!-- A small picture of an image or video from the library, drawn as the
     file becomes visible. Until it arrives, or if the file cannot have one
     (not on this device yet, or not decodable), it shows the kind's icon. -->
<script lang="ts">
  import { request } from "../api";
  import { thumbnailLoader } from "../mediaThumbs";
  import Icon from "./Icon.svelte";
  import type { IconName } from "./icons";

  let { blob, name, icon, width = 96 }: { blob: string; name: string; icon: IconName; width?: number } = $props();

  let src: string | null = $state(null);
  let root: HTMLSpanElement | undefined = $state();
  let wanted = $state(false);

  // Only files that scroll into view are asked for.
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
    load(blob, name, width).then((url) => {
      if (current) src = url;
    });
    return () => {
      current = false;
    };
  });
</script>

<script module lang="ts">
  const load = thumbnailLoader(async (blob, name, width) =>
    request({ area: "Library", request: { MediaThumbnail: { blob, name, width } } }, "Thumbnail"),
  );
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
    width: 3.5rem;
    aspect-ratio: 16 / 9;
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
