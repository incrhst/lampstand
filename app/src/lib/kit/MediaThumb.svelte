<!-- A small picture of an image or video from the library. -->
<script lang="ts">
  import { loadMediaThumb } from "../mediaThumbLoader";
  import LazyThumb from "./LazyThumb.svelte";
  import type { IconName } from "./icons";

  let { blob, name, icon, width = 96 }: { blob: string; name: string; icon: IconName; width?: number } = $props();
</script>

<LazyThumb {icon} load={() => loadMediaThumb(blob, name, width)} />
