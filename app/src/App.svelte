<!-- The shell: a top bar with the two weekly jobs (Plan and Present) and
     Setup apart on the right, the screen, and a status bar whose sync and
     screens items open Setup. Each screen lives in src/routes/<name>/ so
     T47 to T55 can each own one folder. -->
<script lang="ts">
  import PlanView from "./routes/plan/PlanView.svelte";
  import OperateView from "./routes/operate/OperateView.svelte";
  import SetupView, { type SetupTab } from "./routes/setup/SetupView.svelte";
  import { isCalibrated, live, refreshOutputs } from "./lib/state.svelte";
  import Dot from "./lib/kit/Dot.svelte";
  import Icon from "./lib/kit/Icon.svelte";
  import Placeholder from "./lib/kit/Placeholder.svelte";
  import ProgressBar from "./lib/kit/ProgressBar.svelte";
  import StatusBar from "./lib/kit/StatusBar.svelte";

  // The operator view keeps its T47 name in code; volunteers call it Present.
  const screens = [
    { id: "plan", label: "Plan" },
    { id: "operate", label: "Present" },
  ] as const;
  type Screen = (typeof screens)[number]["id"] | "setup";

  let screen: Screen = $state("plan");
  let setupTab: SetupTab = $state("screens");

  function openSetup(tab: SetupTab) {
    setupTab = tab;
    screen = "setup";
  }
  let enabled = $derived(live.outputs.filter((o) => o.enabled).length);
  let uncalibrated = $derived(live.outputs.filter((o) => !isCalibrated(o)).map((o) => o.name));

  refreshOutputs();
</script>

<div class="shell">
  <header>
    <span class="brand"><Icon name="lamp" size={18} />Lampstand</span>
    <nav aria-label="Screens">
      {#each screens as s}
        <button class:active={screen === s.id} aria-current={screen === s.id ? "page" : undefined} onclick={() => (screen = s.id)}>
          {s.label}
        </button>
      {/each}
    </nav>
    <div class="right">
      {#if live.loaded}
        <p class="onair">Live service: <strong>{live.loaded.title}</strong></p>
      {/if}
      <button class="setup" class:active={screen === "setup"} aria-current={screen === "setup" ? "page" : undefined} onclick={() => (screen = "setup")}>
        <Icon name="setup" size={16} />
        Setup
      </button>
    </div>
  </header>

  <main>
    {#if screen === "plan"}
      <PlanView onopened={() => (screen = "operate")} />
    {:else if screen === "operate"}
      <OperateView onsetup={() => openSetup("screens")} />
    {:else}
      <SetupView tab={setupTab} onchange={(tab) => (setupTab = tab)} />
    {/if}
  </main>

  <StatusBar>
    {#snippet start()}
      <button class="status-link" onclick={() => openSetup("devices")} title="Open Setup › Devices">
        <Dot />
        <span>This device only</span>
      </button>
      <Placeholder ticket="T18">Sync</Placeholder>
    {/snippet}
    {#snippet middle()}
      {#if live.progress}
        <span class="num">{live.status}</span>
        <!-- done and total count files, so one file gives no fraction. -->
        <ProgressBar
          value={live.progress.total > 1 ? live.progress.done / live.progress.total : null}
          label="Import progress"
          width="6rem"
        />
      {:else}
        {live.status}
      {/if}
    {/snippet}
    {#snippet end()}
      <button class="status-link" onclick={() => openSetup("screens")} title="Open Setup › Screens">
        <Icon name="monitor" size={14} />
        {live.outputs.length}
        {live.outputs.length === 1 ? "screen" : "screens"}
        {#if live.outputs.length > 0}<span>· {enabled} on</span>{/if}
        {#if uncalibrated.length > 0}<span class="warn">· {uncalibrated.join(", ")} not calibrated</span>{/if}
      </button>
    {/snippet}
  </StatusBar>
</div>

<style>
  .shell {
    display: flex;
    flex-direction: column;
    height: 100vh;
  }
  header {
    flex-shrink: 0;
    height: 3.5rem;
    display: flex;
    align-items: center;
    gap: 1.5rem;
    padding: 0 1.25rem;
    border-bottom: 1px solid var(--line);
    background: var(--panel);
  }
  .brand {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 1.1rem;
    font-weight: 700;
    letter-spacing: -0.01em;
    color: var(--gold);
  }
  nav {
    display: flex;
    gap: 0.25rem;
  }
  nav button {
    font: inherit;
    font-size: 0.875rem;
    font-weight: 500;
    background: none;
    border: none;
    color: var(--muted);
    padding: 0.4rem 0.9rem;
    border-radius: var(--radius);
    cursor: pointer;
    /* No transition: selection shows at once, and a window that is not
       frontmost can freeze a transition part way. */
  }
  nav button:hover {
    color: var(--text);
  }
  nav button.active {
    color: var(--text);
    background: var(--raised);
  }
  .right {
    margin-left: auto;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 1rem;
  }
  .setup {
    font: inherit;
    font-size: 0.875rem;
    font-weight: 500;
    background: none;
    border: none;
    color: var(--muted);
    padding: 0.4rem 0.75rem;
    border-radius: var(--radius);
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    flex-shrink: 0;
  }
  .setup:hover {
    color: var(--text);
  }
  .setup.active {
    color: var(--text);
    background: var(--raised);
  }
  .onair {
    min-width: 0;
    font-size: 0.875rem;
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .onair strong {
    color: var(--text);
    font-weight: 500;
  }
  main {
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }
  .status-link {
    font: inherit;
    color: inherit;
    background: none;
    border: none;
    border-radius: 4px;
    padding: 0.15rem 0.4rem;
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    cursor: pointer;
  }
  .warn {
    color: var(--warn);
  }
  .status-link:hover {
    background: var(--raised);
    color: var(--text);
  }
</style>
