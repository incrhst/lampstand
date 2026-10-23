<!-- The shell: a top bar with the two weekly jobs (Plan and Present) and
     Setup apart on the right, the screen, and a status bar whose sync and
     screens items open Setup. Each screen lives in src/routes/<name>/ so
     T47 to T55 can each own one folder. -->
<script lang="ts">
  import PlanView from "./routes/plan/PlanView.svelte";
  import OperateView from "./routes/operate/OperateView.svelte";
  import SetupView, { type SetupTab } from "./routes/setup/SetupView.svelte";
  import HelpView, { type HelpTab } from "./routes/help/HelpView.svelte";
  import { request } from "./lib/api";
  import { checkForUpdate, isCalibrated, live, refreshObs, refreshOutputs, refreshSettings, refreshSync, refreshThemes } from "./lib/state.svelte";
  import { formatWhen } from "./lib/when";
  import { syncPill } from "./lib/sync";
  import { CONNECT_DEVICE, connectDue, dismissForGood, loadDismissed, loadSnoozed } from "./lib/tips";
  import { WELCOME_TITLE } from "./lib/getStarted";
  import { isPicture, isScreen, obsPill } from "./lib/obs";
  import { modLabel, screenForKey } from "./lib/shortcuts";
  import { isPlanning, withRole } from "./lib/presenting";
  import PresenterNotices from "./lib/kit/PresenterNotices.svelte";
  import { availableVersion, CHECK_EVERY_MS, FIRST_CHECK_MS } from "./lib/updates";
  import { allDone, steps as getStartedSteps, welcomeService, type Facts, type StepId } from "./lib/getStarted";
  import GetStarted from "./lib/kit/GetStarted.svelte";
  import Dot from "./lib/kit/Dot.svelte";
  import Icon from "./lib/kit/Icon.svelte";
  import Logo from "./lib/kit/Logo.svelte";
  import ProgressBar from "./lib/kit/ProgressBar.svelte";
  import StatusBar from "./lib/kit/StatusBar.svelte";

  // The operator view keeps its T47 name in code; volunteers call it Present.
  const screens = [
    { id: "plan", label: "Plan", key: "1" },
    { id: "operate", label: "Present", key: "2" },
  ] as const;
  type Screen = (typeof screens)[number]["id"] | "setup" | "help";

  let screen: Screen = $state("plan");
  let setupTab: SetupTab = $state("screens");
  let helpTab: HelpTab = $state("tips");

  // Mod+1 Plan, Mod+2 Present, Mod+\ the other of the two.
  const mod = modLabel(typeof navigator === "undefined" ? "" : navigator.platform);
  function onkeydown(e: KeyboardEvent) {
    const next = screenForKey(e, screen);
    if (!next) return;
    e.preventDefault();
    screen = next;
  }

  function openSetup(tab: SetupTab) {
    setupTab = tab;
    screen = "setup";
  }
  function openHelp(tab: HelpTab) {
    helpTab = tab;
    screen = "help";
  }
  let obsStatus = $derived(obsPill(live.obs));
  let planningOnly = $derived(isPlanning(live.team));
  let syncStatus = $derived(syncPill(live.sync));
  let rolePill = $derived(withRole(syncStatus, live.team));
  // The quiet "Connect another device" line beside the sync pill, until a device is paired or it is put off.
  let connectLine = $derived.by(() => {
    void live.tipsVersion;
    return connectDue({ paired: live.team?.paired.length, realUse: live.realUse }, loadDismissed(), loadSnoozed());
  });
  // Pairing a device ends the prompt for good, even if it is unpaired later.
  $effect(() => {
    if (live.team && live.team.paired.length > 0) dismissForGood(CONNECT_DEVICE);
  });

  // A new release is looked for shortly after starting and then every few
  // hours, quietly: no network is not worth an error. Installing it is up to
  // the operator, on Help › About.
  let newVersion = $derived(live.update ? availableVersion(live.update.state) : null);
  $effect(() => {
    const first = setTimeout(() => checkForUpdate(), FIRST_CHECK_MS);
    const again = setInterval(() => checkForUpdate(), CHECK_EVERY_MS);
    return () => {
      clearTimeout(first);
      clearInterval(again);
    };
  });
  // Screens are windows and displays; pictures sent to OBS are counted apart.
  let screenOutputs = $derived(live.outputs.filter(isScreen));
  let pictureOutputs = $derived(live.outputs.filter(isPicture));
  let enabled = $derived(screenOutputs.filter((o) => o.enabled).length);
  let picturesOn = $derived(pictureOutputs.filter((o) => o.enabled).length);
  let uncalibrated = $derived(live.outputs.filter((o) => !isCalibrated(o)).map((o) => o.name));

  // The live service's date, beside its title: most services are called
  // "Sunday service". Fetched when a different service is opened.
  let liveWhen: string | null = $state(null);
  $effect(() => {
    const id = live.loaded?.service;
    liveWhen = null;
    if (!id) return;
    request({ area: "Service", request: { Get: id } }, "Service")
      .then((s) => {
        if (live.loaded?.service === id && s.starts_at != null) liveWhen = formatWhen(s.starts_at, s.time_zone);
      })
      .catch(() => {});
  });

  // Get started: what a new install still has to do, from what the app can
  // see. It is shown until everything is done or the operator hides it, and
  // both are remembered on this device.
  const flag = (key: string) => {
    try {
      return localStorage.getItem(key) === "1";
    } catch {
      return false;
    }
  };
  const setFlag = (key: string) => {
    try {
      localStorage.setItem(key, "1");
    } catch {
      // Not remembered this time.
    }
  };
  let hidden = $state(flag("lampstand.getstarted.hidden"));
  let tried = $state(flag("lampstand.getstarted.tried"));
  let told = $state(flag("lampstand.getstarted.told"));
  let songCount = $state(0);
  let serviceList: Facts["services"] = $state([]);
  async function refreshFacts() {
    try {
      songCount = (await request({ area: "Library", request: { SearchSongs: { query: "", limit: 4 } } }, "Songs")).length;
      serviceList = (await request({ area: "Service", request: "List" }, "Services")).map((s) => ({ id: s.id, title: s.title, items: s.items }));
    } catch {
      // The strip just keeps what it knew.
    }
  }
  $effect(() => {
    void live.songsVersion;
    void screen; // Look again as the operator moves about: a service may have been planned.
    refreshFacts();
  });
  $effect(() => {
    if (live.loaded && !tried) {
      tried = true;
      setFlag("lampstand.getstarted.tried");
    }
  });
  $effect(() => {
    live.realUse = tried || serviceList.some((s) => s.title !== WELCOME_TITLE && s.items > 0);
  });
  let started = $derived(getStartedSteps({ screensOn: enabled, songs: songCount, services: serviceList, triedService: tried, brandSet: !!live.settings?.brand }));
  let showStrip = $derived(!hidden && !allDone(started));
  function hide() {
    hidden = true;
    setFlag("lampstand.getstarted.hidden");
  }
  $effect(() => {
    // Everything is done: it has done its job.
    if (allDone(started) && !hidden) hide();
  });
  /** Opens the optional "tell us about your church" form in the browser, and stops asking. */
  async function tellUs() {
    try {
      await request({ area: "About", request: "TellUs" }, "Ok");
      told = true;
      setFlag("lampstand.getstarted.told");
    } catch (e) {
      live.status = e instanceof Error ? e.message : String(e);
    }
  }
  async function startStep(id: StepId) {
    if (id === "try") {
      const welcome = welcomeService(serviceList);
      if (live.loaded) {
        screen = "operate";
      } else if (welcome) {
        try {
          live.planning = welcome.id;
          live.loaded = await request({ area: "Service", request: { Open: welcome.id } }, "Loaded");
          screen = "operate";
        } catch (e) {
          live.status = e instanceof Error ? e.message : String(e);
        }
      } else {
        screen = "plan";
      }
    } else if (id === "look") {
      openSetup("brand");
    } else if (id === "screen") {
      openSetup("screens");
    } else {
      screen = "plan";
    }
  }

  refreshOutputs();
  refreshThemes();
  refreshSettings();
  refreshObs();
  refreshSync();
</script>

<svelte:window {onkeydown} />

<div class="shell">
  <header>
    <span class="brand"><Logo size={22} />Lampstand</span>
    <nav aria-label="Screens">
      {#each screens as s}
        <button class:active={screen === s.id} aria-current={screen === s.id ? "page" : undefined} title="{s.label} ({mod}{s.key}; {mod}\ switches between Plan and Present)" onclick={() => (screen = s.id)}>
          {s.label}
        </button>
      {/each}
    </nav>
    <div class="right">
      <!-- Present's own bar already names the service, so it is not said twice there. -->
      {#if live.loaded && screen !== "operate"}
        <p class="onair">
          <span class="label">Live service:</span>
          <strong title={live.loaded.title}>{live.loaded.title}</strong>
          {#if liveWhen}<span class="when">· {liveWhen}</span>{/if}
        </p>
      {/if}
      <button class="setup" class:active={screen === "help"} aria-current={screen === "help" ? "page" : undefined} onclick={() => (screen = "help")}>
        <Icon name="info" size={16} />
        Help
      </button>
      <button class="setup" class:active={screen === "setup"} aria-current={screen === "setup" ? "page" : undefined} onclick={() => (screen = "setup")}>
        <Icon name="setup" size={16} />
        Setup
      </button>
    </div>
  </header>

  <PresenterNotices />

  {#if showStrip}<GetStarted steps={started} onstep={startStep} ontell={told ? undefined : tellUs} onhide={hide} />{/if}

  <main>
    {#if screen === "plan"}
      <PlanView onopened={() => (screen = "operate")} />
    {:else if screen === "operate"}
      <OperateView ondevices={() => openSetup("devices")} onsetup={() => openSetup("screens")} onidle={() => openSetup("idle")} onobs={() => openSetup("obs")} ontips={() => openHelp("tips")} onplan={() => (screen = "plan")} />
    {:else if screen === "help"}
      <HelpView tab={helpTab} onchange={(tab) => (helpTab = tab)} />
    {:else}
      <SetupView tab={setupTab} onchange={(tab) => (setupTab = tab)} />
    {/if}
  </main>

  <StatusBar>
    {#snippet start()}
      <button class="status-link" onclick={() => openSetup("devices")} title="{rolePill.title}. Open Setup › Devices">
        <Dot tone={syncStatus.tone} />
        <span class:warn={syncStatus.tone === "warn"}>{rolePill.text}</span>
      </button>
      {#if connectLine}
        <button class="status-link quiet" onclick={() => openSetup("devices")} title="Pair another computer to keep plans and screens in step. Open Setup › Devices">
          Connect another device
        </button>
      {/if}
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
      {#if newVersion}
        <button class="status-link" onclick={() => openHelp("about")} title="A new version of Lampstand is ready to install. Open Help › About">
          <Icon name="download" size={14} />
          <span class="update">Update {newVersion} available</span>
        </button>
      {/if}
      {#if obsStatus}
        <!-- OBS's connection, streaming and recording, kept quiet here;
             Present has the OBS control itself. -->
        <button class="status-link" onclick={() => openSetup("obs")} title="{obsStatus.title}. Open Setup › OBS">
          <Dot tone={obsStatus.tone} />
          <span class:warn={obsStatus.tone === "warn"}>{obsStatus.text}</span>
        </button>
      {/if}
      <button class="status-link" onclick={() => openSetup("screens")} title="Open Setup › Screens">
        <Icon name="monitor" size={14} />
        {screenOutputs.length}
        {screenOutputs.length === 1 ? "screen" : "screens"}
        {#if screenOutputs.length > 0}<span>· {enabled} on{planningOnly && enabled > 0 ? ", dark while planning" : ""}</span>{/if}
        {#if uncalibrated.length > 0}<span class="warn">· {uncalibrated.join(", ")} not calibrated</span>{/if}
      </button>
      {#if pictureOutputs.length > 0}
        <button class="status-link" onclick={() => openSetup("obs")} title="Pictures sent to OBS as a Browser source. Open Setup › OBS">
          <Icon name="broadcast" size={14} />
          {pictureOutputs.length}
          {pictureOutputs.length === 1 ? "picture" : "pictures"} to OBS
          <span>· {picturesOn} on</span>
        </button>
      {/if}
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
  /* A long title truncates; the label and the date keep their width. */
  .onair {
    min-width: 0;
    display: flex;
    gap: 0.3rem;
    font-size: 0.875rem;
    color: var(--muted);
    white-space: nowrap;
  }
  .onair .label,
  .onair .when {
    flex-shrink: 0;
  }
  .onair strong {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--text);
    font-weight: 500;
  }
  main {
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }
  .update {
    color: var(--gold);
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
  .status-link.quiet {
    text-decoration: underline;
    text-decoration-color: var(--line-strong);
    text-underline-offset: 2px;
  }
  .status-link:hover {
    background: var(--raised);
    color: var(--text);
  }
</style>
