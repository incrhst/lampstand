<!-- A small picture of a slide deck's first slide. -->
<script module lang="ts">
  import { request } from "../api";
  import { thumbnailLoader } from "../mediaThumbs";

  // Asked by deck, and again if the deck changes: the key carries the
  // version (the blob the deck's current file is stored as).
  const loadDeck = thumbnailLoader(async (key, _name, width) =>
    request({ area: "Library", request: { DeckThumbnail: { deck: key.split(":")[0], width } } }, "Thumbnail"),
  );
</script>

<script lang="ts">
  import LazyThumb from "./LazyThumb.svelte";

  let { deck, version = "", width = 160 }: { deck: string; version?: string; width?: number } = $props();
</script>

<LazyThumb icon="deck" load={() => loadDeck(`${deck}:${version}`, "", width)} />
