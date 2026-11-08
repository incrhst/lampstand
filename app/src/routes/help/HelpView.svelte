<!-- Help: what Lampstand can do and where to get answers, apart from Setup,
     which is only for configuration. Quick tips, the keys, the docs and
     feedback, and About (the version, and updating it). -->
<script module lang="ts">
  export type HelpTab = "tips" | "keys" | "feedback" | "about";
</script>

<script lang="ts">
  import Icon from "../../lib/kit/Icon.svelte";
  import ListRow from "../../lib/kit/ListRow.svelte";
  import TipsView from "./TipsView.svelte";
  import KeysView from "./KeysView.svelte";
  import FeedbackView from "./FeedbackView.svelte";
  import AboutView from "./AboutView.svelte";

  let { tab, onchange }: { tab: HelpTab; onchange: (tab: HelpTab) => void } = $props();

  const tabs = [
    { id: "tips", label: "Quick tips", subtitle: "What Lampstand can do", icon: "lamp" },
    { id: "keys", label: "Keys", subtitle: "Run Present from the keyboard", icon: "keyboard" },
    { id: "feedback", label: "Help and feedback", subtitle: "Docs, questions, problems, ideas", icon: "info" },
    { id: "about", label: "About", subtitle: "Version and updates", icon: "sync" },
  ] as const;
</script>

<div class="help">
  <aside class="pane pane-side side" aria-label="Help">
    <div class="pane-head"><h2>Help</h2></div>
    <ul class="plain pane-body">
      {#each tabs as t (t.id)}
        <ListRow title={t.label} subtitle={t.subtitle} active={tab === t.id} onclick={() => onchange(t.id)}>
          {#snippet leading()}<Icon name={t.icon} />{/snippet}
        </ListRow>
      {/each}
    </ul>
  </aside>

  <div class="content">
    {#if tab === "tips"}
      <TipsView />
    {:else if tab === "keys"}
      <KeysView />
    {:else if tab === "feedback"}
      <FeedbackView />
    {:else}
      <AboutView />
    {/if}
  </div>
</div>

<style>
  .help {
    display: grid;
    grid-template-columns: 248px minmax(0, 1fr);
    height: 100%;
    min-height: 0;
  }
  .side {
    border-right: 1px solid var(--line);
  }
  .content {
    overflow-y: auto;
    min-width: 0;
  }
</style>
