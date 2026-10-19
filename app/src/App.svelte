<!-- The shell: a tab per screen and a status bar. Each screen lives in
     src/routes/<name>/ so T47 to T55 can each own one folder. -->
<script lang="ts">
  import PlanView from "./routes/plan/PlanView.svelte";
  import OperateView from "./routes/operate/OperateView.svelte";
  import OutputsView from "./routes/outputs/OutputsView.svelte";
  import { live } from "./lib/state.svelte";

  const screens = [
    { id: "plan", label: "Plan" },
    { id: "operate", label: "Operate" },
    { id: "outputs", label: "Outputs" },
  ] as const;
  type Screen = (typeof screens)[number]["id"];

  let screen: Screen = $state("plan");
</script>

<nav aria-label="Screens">
  <strong class="brand">Lampstand</strong>
  {#each screens as s}
    <button class:active={screen === s.id} aria-current={screen === s.id ? "page" : undefined} onclick={() => (screen = s.id)}>
      {s.label}
    </button>
  {/each}
  {#if live.loaded}
    <span class="muted onair-title">Open: {live.loaded.title}</span>
  {/if}
</nav>

<main>
  {#if screen === "plan"}
    <PlanView onopened={() => (screen = "operate")} />
  {:else if screen === "operate"}
    <OperateView />
  {:else}
    <OutputsView />
  {/if}
</main>

<footer aria-live="polite">{live.status}</footer>

<style>
  nav {
    display: flex;
    gap: 0.25rem;
    align-items: center;
    padding: 0.5rem 1rem;
    border-bottom: 1px solid var(--line);
    background: var(--panel);
  }
  .brand {
    margin-right: 1rem;
    color: var(--accent);
  }
  nav button {
    font: inherit;
    background: none;
    border: none;
    color: var(--muted);
    padding: 0.4rem 0.8rem;
    border-radius: 6px;
    cursor: pointer;
  }
  nav button.active {
    color: var(--text);
    background: var(--raised);
  }
  nav button:focus-visible {
    outline: 2px solid var(--accent);
  }
  .onair-title {
    margin-left: auto;
  }
  main {
    padding: 1rem;
  }
  footer {
    position: fixed;
    bottom: 0;
    left: 0;
    right: 0;
    padding: 0.3rem 1rem;
    font-size: 0.85rem;
    color: var(--muted);
    background: var(--panel);
    border-top: 1px solid var(--line);
    min-height: 1.2rem;
  }
</style>
