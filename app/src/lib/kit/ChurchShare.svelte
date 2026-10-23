<!-- The note on pages whose settings follow the church to its other devices
     (Church look, the idle screen): "Shared with your other devices" or
     "This device only", the files still to arrive, and, once, the choice
     between two looks that were made apart. -->
<script lang="ts">
  import { request, type ChurchChoice, type ChurchStatus } from "../api";
  import { sharingNote, waitingText } from "../church";
  import { live, refreshSettings, refreshThemes, report } from "../state.svelte";
  import Banner from "./Banner.svelte";
  import Button from "./Button.svelte";
  import Icon from "./Icon.svelte";

  /** Which awaited files to mention on this page (all if unset). */
  let { only }: { only?: readonly string[] } = $props();

  let paired = $state(0);
  let status = $state<ChurchStatus | null>(null);
  let busy = $state(false);

  async function load() {
    try {
      paired = (await request({ area: "Team", request: "Status" }, "Team")).paired.length;
      status = await request({ area: "Settings", request: "Church" }, "Church");
    } catch {
      // Sync is off or the demo has none: the note says this device only.
    }
  }

  $effect(() => {
    live.churchVersion;
    load();
  });

  async function choose(choice: ChurchChoice) {
    try {
      busy = true;
      status = await request({ area: "Settings", request: { ResolveChurch: choice } }, "Church");
      await refreshSettings();
      await refreshThemes();
    } catch (e) {
      report(e);
    } finally {
      busy = false;
    }
  }

  let waiting = $derived(status ? waitingText(status.waiting, only) : null);
</script>

{#if status?.conflict}
  <Banner tone="warn" icon="sync">
    <p class="ask">
      Use this device's look, or the one from {status.conflict.other_device}?
    </p>
    <p class="small faint">This device: {status.conflict.mine || "nothing"}. {status.conflict.other_device}: {status.conflict.theirs || "nothing"}.</p>
    <div class="choices">
      <Button size="small" variant="primary" disabled={busy} onclick={() => choose("ThisDevice")}>Use this device's</Button>
      <Button size="small" disabled={busy} onclick={() => choose("Shared")}>Use the one from {status.conflict.other_device}</Button>
    </div>
  </Banner>
{/if}
{#if waiting}
  <Banner tone="neutral"><span>{waiting}…</span></Banner>
{/if}
<p class="note faint small"><Icon name="sync" size={14} />{sharingNote(paired, status)}</p>

<style>
  .note {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    margin: 0.5rem 0;
  }
  .ask {
    font-weight: 600;
  }
  .choices {
    display: flex;
    gap: 0.5rem;
    margin-top: 0.5rem;
    flex-wrap: wrap;
  }
</style>
