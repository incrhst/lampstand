<!-- One icon from icons.ts. Decorative unless given a label. -->
<script lang="ts">
  import { icons, type IconName } from "./icons";

  let { name, size = 16, label }: { name: IconName; size?: number; label?: string } = $props();

  // The markup is ours (icons.ts), never user text.
  let svg = $derived(
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="${size}" height="${size}" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">${icons[name]}</svg>`,
  );
</script>

<span class="icon" role={label ? "img" : undefined} aria-label={label} aria-hidden={label ? undefined : "true"}>
  {@html svg}
</span>

<style>
  .icon {
    display: inline-flex;
    flex-shrink: 0;
    line-height: 0;
  }
</style>
