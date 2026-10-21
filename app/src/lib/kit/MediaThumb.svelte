<!-- A small picture of an image or video from the library. -->
<script module lang="ts">
  import { request } from "../api";
  import { thumbnailLoader } from "../mediaThumbs";

  const loadMedia = thumbnailLoader(async (blob, name, width) =>
    request({ area: "Library", request: { MediaThumbnail: { blob, name, width } } }, "Thumbnail"),
  );
</script>

<script lang="ts">
  import LazyThumb from "./LazyThumb.svelte";
  import type { IconName } from "./icons";

  let { blob, name, icon, width = 96 }: { blob: string; name: string; icon: IconName; width?: number } = $props();
</script>

<LazyThumb {icon} load={() => loadMedia(blob, name, width)} />
