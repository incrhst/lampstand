<!-- Why OBS isn't connected and what to do about it (T89): Open OBS, where
     to get it, or the steps to turn its WebSocket server on. Then the live
     item's scene (T90): a switch that failed, or a hand pick holding, with
     Resume. Shows nothing unless OBS matters on this device (lib/obs.ts). -->
<script lang="ts">
  import type { ObsView } from "../../lib/api";
  import { notice, sceneNotice } from "../../lib/obs";
  import { obs, report } from "../../lib/state.svelte";
  import Banner from "../../lib/kit/Banner.svelte";
  import Button from "../../lib/kit/Button.svelte";

  /** `onsetup` opens Setup › OBS; leave it out on that screen itself. */
  let { view, onsetup }: { view: ObsView | null; onsetup?: () => void } = $props();

  let n = $derived(notice(view));
  let scene = $derived(sceneNotice(view));

  async function resume() {
    try {
      await obs("Resume");
    } catch (e) {
      report(e);
    }
  }
  let opening = $state(false);

  async function download() {
    try {
      await obs("Download");
    } catch (e) {
      report(e);
    }
  }

  async function open() {
    opening = true;
    try {
      await obs("Launch");
    } catch (e) {
      report(e);
    }
    // OBS takes a few seconds to start; the connection picks it up.
    setTimeout(() => (opening = false), 4000);
  }
</script>

{#if n}
  <Banner tone="warn">
    <div class="text">
      <span>{n.message}</span>
      {#if n.steps.length > 0}
        <ol>
          {#each n.steps as step (step)}<li>{step}</li>{/each}
        </ol>
      {/if}
    </div>
    {#if n.action === "open"}
      <Button size="small" icon="play" onclick={open} disabled={opening}>{opening ? "Opening OBS…" : "Open OBS"}</Button>
    {:else if n.action === "get"}
      <Button size="small" icon="download" onclick={download}>Get OBS</Button>
    {:else if n.action === "setup" && onsetup}
      <Button size="small" icon="setup" onclick={onsetup}>OBS settings</Button>
    {/if}
  </Banner>
{/if}
{#if scene}
  <Banner tone={scene.tone} icon="play">
    <span>{scene.message}</span>
    {#if scene.resume}<Button size="small" onclick={resume}>Resume</Button>{/if}
  </Banner>
{/if}

<style>
  .text {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
  }
  ol {
    margin: 0;
    padding-left: 1.2rem;
    font-size: 0.8125rem;
    line-height: 1.5;
  }
</style>
