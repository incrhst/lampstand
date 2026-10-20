<!-- The library's Media tab: images and video already used in a service,
     to add again, and a button to add a file from disk. Media has no
     library document of its own; the services are its library. -->
<script lang="ts">
  import type { MediaFile } from "../../lib/api";
  import { isVideo, mediaKind } from "../../lib/media";
  import Button from "../../lib/kit/Button.svelte";
  import Icon from "../../lib/kit/Icon.svelte";
  import ListRow from "../../lib/kit/ListRow.svelte";
  import Placeholder from "../../lib/kit/Placeholder.svelte";

  let {
    media,
    inService,
    canAdd,
    busy,
    onadd,
    onpick,
  }: {
    media: MediaFile[];
    /** Blobs already in the selected service. */
    inService: Set<string>;
    canAdd: boolean;
    busy: boolean;
    onadd: (file: MediaFile) => void;
    onpick: () => void;
  } = $props();

  const subtitle = (m: MediaFile) => (m.on_device ? mediaKind(m.name) : `${mediaKind(m.name)} · not on this device yet`);
</script>

<ul class="plain pane-body">
  {#each media as file (file.blob)}
    <ListRow title={file.name} subtitle={subtitle(file)} dim={!file.on_device}>
      {#snippet leading()}<Icon name={isVideo(file.name) ? "media" : "announcements"} />{/snippet}
      {#snippet details()}
        {#if isVideo(file.name)}<span class="later"><Placeholder ticket="T27">Plays on screen</Placeholder></span>{/if}
      {/snippet}
      {#snippet trailing()}
        {#if inService.has(file.blob)}
          <span class="in-service"><Icon name="check" size={14} />In service</span>
        {:else}
          <Button size="small" icon="plus" onclick={() => onadd(file)} disabled={!canAdd} label="Add {file.name} to service">
            Add
          </Button>
        {/if}
      {/snippet}
    </ListRow>
  {:else}
    <li class="empty muted">
      <p>No media yet.</p>
      <p>Add a video or an image. Ones used in any service show here to add again.</p>
      <p>Images show on screen now; video plays once playback lands (T27).</p>
    </li>
  {/each}
</ul>

<div class="foot">
  <Button size="small" icon="upload" onclick={onpick} disabled={busy || !canAdd}>Add video or image…</Button>
</div>

<style>
  .empty {
    padding: 2rem 0.75rem;
    text-align: center;
    font-size: 0.875rem;
  }
  .empty p + p {
    margin-top: 0.25rem;
    font-size: 0.8rem;
  }
  .in-service {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    height: 1.75rem;
    padding: 0 0.4rem;
  }
  .in-service :global(.icon) {
    color: var(--ready-text);
  }
  .later {
    margin-top: 0.3rem;
  }
  .foot {
    border-top: 1px solid var(--line);
    padding: 0.75rem 1rem 1rem;
    display: flex;
    gap: 0.5rem;
  }
</style>
