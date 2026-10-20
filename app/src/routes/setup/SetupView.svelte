<!-- Setup: what is set up once and checked before a service, apart from
     the weekly work in Plan and Present. Screens are the outputs this
     device drives; Devices pairs and syncs this machine with others.
     Themes (T51) and Team (T54) join them here. -->
<script module lang="ts">
  export type SetupTab = "screens" | "devices";
</script>

<script lang="ts">
  import Icon from "../../lib/kit/Icon.svelte";
  import ListRow from "../../lib/kit/ListRow.svelte";
  import OutputsView from "../outputs/OutputsView.svelte";
  import DevicesView from "../devices/DevicesView.svelte";

  let { tab, onchange }: { tab: SetupTab; onchange: (tab: SetupTab) => void } = $props();

  const tabs = [
    { id: "screens", label: "Screens", subtitle: "Projectors, TVs and windows", icon: "monitor" },
    { id: "devices", label: "Devices", subtitle: "Pairing and sync", icon: "sync" },
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
      <OutputsView />
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
