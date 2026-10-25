<!-- Setup: what is set up once and checked before a service, apart from
     the weekly work in Plan and Present. Screens are the outputs this
     device drives; Devices pairs and syncs this machine with others.
     OBS is where pictures are sent to OBS and where OBS is controlled (T89).
     Themes (T51) holds the library's themes and Church look (the brand
     quick start) makes a first one; Team (T54) joins them here. -->
<script module lang="ts">
  export type SetupTab = "screens" | "themes" | "brand" | "idle" | "scripture" | "devices" | "obs" | "sound" | "computer";
</script>

<script lang="ts">
  import Icon from "../../lib/kit/Icon.svelte";
  import ListRow from "../../lib/kit/ListRow.svelte";
  import OutputsView from "../outputs/OutputsView.svelte";
  import DevicesView from "../devices/DevicesView.svelte";
  import ObsSetup from "./ObsSetup.svelte";
  import ScriptureSetup from "./ScriptureSetup.svelte";
  import ThemesSetup from "./ThemesSetup.svelte";
  import BrandSetup from "./BrandSetup.svelte";
  import IdleSetup from "./IdleSetup.svelte";
  import SoundSetup from "./SoundSetup.svelte";
  import ComputerSetup from "./ComputerSetup.svelte";
  import { words as soundWords } from "../../lib/sound";
  import { words as computerWords } from "../../lib/computer";

  let { tab, onchange }: { tab: SetupTab; onchange: (tab: SetupTab) => void } = $props();

  const tabs = [
    { id: "screens", label: "Screens", subtitle: "Projectors, TVs and windows", icon: "monitor" },
    { id: "themes", label: "Themes", subtitle: "How songs and verses look", icon: "palette" },
    { id: "brand", label: "Church look", subtitle: "Name, logo, colours, font", icon: "image" },
    { id: "idle", label: "Idle screen", subtitle: "What shows when nothing is on air", icon: "monitor" },
    { id: "scripture", label: "Scripture", subtitle: "Bible versions and the default", icon: "scripture" },
    { id: "devices", label: "Devices", subtitle: "Pairing and sync", icon: "sync" },
    { id: "obs", label: "OBS", subtitle: "Send pictures, control scenes", icon: "broadcast" },
    { id: "sound", label: soundWords.setupTab, subtitle: soundWords.setupTabSubtitle, icon: "sound" },
    { id: "computer", label: computerWords.tab, subtitle: computerWords.tabSubtitle, icon: "setup" },
  ] as const;
</script>

<div class="setup">
  <aside class="pane pane-side side" aria-label="Setup">
    <div class="pane-head"><h2>Setup</h2></div>
    <ul class="plain pane-body">
      {#each tabs as t (t.id)}
        <ListRow title={t.label} subtitle={t.subtitle} active={tab === t.id} onclick={() => onchange(t.id)}>
          {#snippet leading()}<Icon name={t.icon} />{/snippet}
        </ListRow>
      {/each}
    </ul>
  </aside>

  <div class="content">
    {#if tab === "screens"}
      <OutputsView onobs={() => onchange("obs")} onidle={() => onchange("idle")} />
    {:else if tab === "themes"}
      <ThemesSetup onbrand={() => onchange("brand")} />
    {:else if tab === "brand"}
      <BrandSetup onthemes={() => onchange("themes")} />
    {:else if tab === "idle"}
      <IdleSetup onscreens={() => onchange("screens")} />
    {:else if tab === "obs"}
      <ObsSetup />
    {:else if tab === "sound"}
      <SoundSetup />
    {:else if tab === "computer"}
      <ComputerSetup />
    {:else if tab === "scripture"}
      <ScriptureSetup />
    {:else}
      <div class="page">
        <h1>Devices</h1>
        <p class="muted intro">
          Pair this machine with the other laptops and office PCs your team plans on, so a change made during the week
          reaches the presenting machine.
        </p>
        <DevicesView />
      </div>
    {/if}
  </div>
</div>

<style>
  .setup {
    display: grid;
    grid-template-columns: 248px minmax(0, 1fr);
    height: 100%;
  }
  .side {
    border-right: 1px solid var(--line);
  }
  .content {
    min-width: 0;
    min-height: 0;
    overflow-y: auto;
  }
  .page {
    max-width: 64rem;
    margin: 0 auto;
    padding: 2rem;
  }
  h1 {
    font-size: 1.3rem;
    font-weight: 600;
    letter-spacing: -0.01em;
  }
  .intro {
    margin: 0.3rem 0 1.5rem;
    max-width: 40rem;
    font-size: 0.875rem;
    line-height: 1.6;
  }
</style>
