<!-- The shell: a top bar with the two weekly jobs (Plan and Present) and
     Setup apart on the right, the screen, and a status bar whose sync and
     screens items open Settings. Each screen lives in src/routes/<name>/ so
     T47 to T55 can each own one folder. -->
<script lang="ts">
  import PlanView from "./routes/plan/PlanView.svelte";
  import OperateView from "./routes/operate/OperateView.svelte";
  import SetupView, { type SetupTab } from "./routes/setup/SetupView.svelte";
  import HelpView, { type HelpTab } from "./routes/help/HelpView.svelte";
  import { request } from "./lib/api";
  import { checkForUpdate, isCalibrated, live, report, refreshObs, refreshOutputs, refreshSettings, refreshSync, refreshThemes } from "./lib/state.svelte";
  import { formatWhen } from "./lib/when";
  import { syncPill } from "./lib/sync";
  import { CONNECT_DEVICE, connectDue, dismissForGood, loadDismissed, loadSnoozed } from "./lib/tips";
  import { WELCOME_TITLE } from "./lib/getStarted";
  import { isPicture, isScreen, obsPill } from "./lib/obs";
  import { modLabel, screenForKey } from "./lib/shortcuts";
  import { isPlanning } from "./lib/presenting";
  import { heldRoles, streamOfflineLine, uncheckedRoles, withRoster } from "./lib/roles";
  import RoleBar from "./lib/kit/RoleBar.svelte";
  import PresenterNotices from "./lib/kit/PresenterNotices.svelte";
  import { drivenBy, isDriving } from "./lib/driving";
  import { availableVersion, CHECK_EVERY_MS, FIRST_CHECK_MS } from "./lib/updates";
  import { allDone, steps as getStartedSteps, welcomeService, type Facts, type StepId } from "./lib/getStarted";
  import GetStarted from "./lib/kit/GetStarted.svelte";
  import Dot from "./lib/kit/Dot.svelte";
  import Icon from "./lib/kit/Icon.svelte";
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
    if (next) {
      e.preventDefault();
      screen = next;
      live.pointTabs = false;
      return;
    }
  }

  // Close service sits beside the live service's name, in Plan and Present,
  // so it never scrolls away or hides with Library & controls. It asks
  // first while something is on air.
  let confirmClose = $state(false);
  function askClose() {
    if (live.live || live.overlay !== "None") confirmClose = true;
    else closeService();
  }
  async function closeService() {
    const loaded = live.loaded;
    confirmClose = false;
    if (!loaded) return;
    try {
      await request({ area: "Service", request: { Close: loaded.service } }, "Ok");
      live.loaded = null;
    } catch (e) {
      report(e);
    }
  }
  $effect(() => {
    if (!live.loaded) confirmClose = false;
  });


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
  // Driving, or being driven, opens Present at once: that is where the job is.
  let wasBusy = false;
  let arriving = $state(false);
  $effect(() => {
    const busy = isDriving(live.team) || drivenBy(live.team) !== null;
    if (busy && !wasBusy) {
      screen = "operate";
      // A short slide-in and a gold edge, so the change is obvious.
      arriving = true;
      setTimeout(() => (arriving = false), 1400);
    }
    wasBusy = busy;
  });
  let syncStatus = $derived(syncPill(live.sync));
  let rolePill = $derived(withRoster(syncStatus, live.team));
  // Set once the OBS cue work is in; false until then, so the line stays away.
  let streamOffline = $derived(streamOfflineLine(live.team));
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
      // Training services (#190) are not the church's own, so they never count as planning one.
      serviceList = (await request({ area: "Service", request: "List" }, "Services"))
        .filter((s) => !s.training)
        .map((s) => ({ id: s.id, title: s.title, items: s.items }));
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
  <!-- The bar always shows, so Plan | Present and the Library & controls
       button are where a volunteer left them, even with the sidebar hidden. -->
  <header>
    <nav aria-label="Screens">
      {#each screens as s}
        <button class:active={screen === s.id} aria-current={screen === s.id ? "page" : undefined} title="{s.label} ({mod}{s.key}; {mod}\ switches between Plan and Present)" onclick={() => { screen = s.id; live.pointTabs = false; }}>
          {s.label}
        </button>
      {/each}
      {#if live.pointTabs && (screen === "plan" || screen === "operate")}
        <!-- At every launch: where a monthly volunteer starts. Goes once a
             tab is chosen, a service is made, or it is closed. -->
        <div class="point-tabs" role="note">
          <div class="point-say">
            <strong>Start here</strong>
            <span><b>Plan</b> gets a service ready · <b>Present</b> runs it on Sunday</span>
          </div>
          <button type="button" class="point-close" onclick={() => (live.pointTabs = false)} aria-label="Close Start here" title="Close">×</button>
        </div>
      {/if}
    </nav>
    <div class="right">
      {#if live.loaded}
        <!-- While Close service asks, the question takes the name's place. -->
        {#if !confirmClose}
        <p class="onair">
          <span class="label">Live service:</span>
          <strong title={live.loaded.title}>{live.loaded.title}</strong>
          {#if liveWhen}<span class="when">· {liveWhen}</span>{/if}
        </p>
        {/if}
        {#if confirmClose}
          <span class="close-ask" role="alert">
            Something is on air. Close the service?
            <button class="close-live danger" onclick={closeService}>Close service</button>
            <button class="close-live" onclick={() => (confirmClose = false)}>Keep open</button>
          </span>
        {:else}
          <button class="close-live" onclick={askClose} title="Put the service away; it stays in Plan">
            <Icon name="x" size={14} />Close service
          </button>
        {/if}
      {/if}
      <button class="setup" class:active={screen === "help"} aria-current={screen === "help" ? "page" : undefined} title="Help" onclick={() => (screen = "help")}>
        <Icon name="info" size={16} />
        <span class="setup-word">Help</span>
        {#if newVersion}<span class="update-dot" title="Update {newVersion} available" role="img" aria-label="Update available"></span>{/if}
      </button>
      <button class="setup" class:active={screen === "setup"} aria-current={screen === "setup" ? "page" : undefined} title="Settings" onclick={() => (screen = "setup")}>
        <Icon name="setup" size={16} />
        <span class="setup-word">Settings</span>
      </button>
    </div>
  </header>

  <PresenterNotices />

  {#if showStrip}<GetStarted steps={started} onstep={startStep} ontell={told ? undefined : tellUs} onhide={hide} />{/if}

  <main class:arriving>
    {#if screen === "plan"}
      <PlanView onopened={() => (screen = "operate")} onscreens={() => openSetup("screens")} />
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
      <button class="status-link" onclick={() => openSetup("devices")} title="{rolePill.title}. Open Settings › Devices">
        <Dot tone={syncStatus.tone} />
        {#if live.team && live.team.paired.length > 0}
          <RoleBar held={heldRoles(live.team)} unchecked={uncheckedRoles(live.team)} />
        {/if}
        <span class:warn={syncStatus.tone === "warn"}>{rolePill.text}</span>
      </button>
      <!-- Always in the page, so a screen reader announces the line when it fills in, once. -->
      <span class="stream-offline" role="status">
        {#if streamOffline}<Dot tone="warn" /><span class="warn">{streamOffline}</span>{/if}
      </span>
      {#if connectLine}
        <button class="status-link quiet" onclick={() => openSetup("devices")} title="Pair another computer to keep plans and screens in step. Open Settings › Devices">
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
        <button class="status-link update-pill" onclick={() => openHelp("about")} title="A new version of Lampstand is ready to install. Open Help › About">
          <Icon name="download" size={14} />
          <span class="update">Update {newVersion} available</span>
        </button>
      {/if}
      {#if obsStatus}
        <!-- OBS's connection, streaming and recording, kept quiet here;
             Present has the OBS control itself. -->
        <button class="status-link" onclick={() => openSetup("obs")} title="{obsStatus.title}. Open Settings › OBS">
          <Dot tone={obsStatus.tone} />
          <span class:warn={obsStatus.tone === "warn"}>{obsStatus.text}</span>
        </button>
      {/if}
      <button class="status-link" onclick={() => openSetup("screens")} title="Open Settings › Screens">
        <Icon name="monitor" size={14} />
        {screenOutputs.length}
        {screenOutputs.length === 1 ? "screen" : "screens"}
        {#if screenOutputs.length > 0}<span>· {enabled} on{planningOnly && enabled > 0 ? ", dark here (this device does not run the screens)" : ""}</span>{/if}
        {#if uncalibrated.length > 0}<span class="warn">· {uncalibrated.join(", ")} not calibrated</span>{/if}
      </button>
      {#if pictureOutputs.length > 0}
        <button class="status-link" onclick={() => openSetup("obs")} title="Pictures sent to OBS as a Browser source. Open Settings › OBS">
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
  nav {
    position: relative;
    display: flex;
    gap: 0.25rem;
  }
  /* Start here, under the Plan | Present tabs: a soft yellow note with a
     dark outline and a small point at its top left, toward the tabs. Still,
     no animation. */
  .point-tabs {
    --note: #fbeca2;
    --note-line: #1f1f1f;
    position: absolute;
    top: calc(100% + 0.75rem);
    left: 0.25rem;
    z-index: 40;
    display: flex;
    align-items: flex-start;
    gap: 0.6rem;
    width: max-content;
    max-width: 24rem;
    padding: 0.7rem 0.6rem 0.7rem 0.9rem;
    border: 3px solid var(--note-line);
    border-radius: 12px;
    background: var(--note);
    color: #2a2410;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.45);
    font-size: 0.8125rem;
  }
  /* The point: a small square turned on its corner, outlined on its two
     outer sides only, so it joins the note's edge. */
  .point-tabs::before {
    content: "";
    position: absolute;
    top: -9px;
    left: 1.6rem;
    width: 14px;
    height: 14px;
    background: var(--note);
    border-top: 3px solid var(--note-line);
    border-left: 3px solid var(--note-line);
    border-radius: 3px 0 0 0;
    transform: rotate(45deg);
  }
  .point-say {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    color: #4a4020;
  }
  .point-say strong {
    color: #1f1a08;
    font-size: 0.875rem;
  }
  .point-say b {
    color: #1f1a08;
    font-weight: 700;
  }
  .point-close {
    font: inherit;
    font-size: 1rem;
    line-height: 1;
    color: #4a4020;
    background: none;
    border: none;
    padding: 0 0.2rem;
    cursor: pointer;
  }
  .point-close:hover {
    color: #000;
  }
  /* Plan and Present as quiet tabs: the chosen one in full white with a
     thin gold line on the bar's bottom edge, the other muted. */
  nav {
    align-self: stretch;
  }
  nav button {
    position: relative;
    font: inherit;
    font-size: 0.9375rem;
    font-weight: 500;
    background: none;
    border: none;
    color: var(--muted);
    padding: 0 0.95rem;
    cursor: pointer;
    /* No transition: selection shows at once, and a window that is not
       frontmost can freeze a transition part way. */
  }
  nav button:hover {
    color: var(--text);
  }
  nav button::after {
    content: "";
    position: absolute;
    left: 0.6rem;
    right: 0.6rem;
    bottom: -1px;
    height: 2px;
    border-radius: 2px 2px 0 0;
    background: transparent;
  }
  nav button:hover::after {
    background: var(--line-strong);
  }
  nav button.active {
    color: var(--text);
    font-weight: 600;
  }
  nav button.active::after {
    background: var(--gold);
  }
  .right {
    margin-left: auto;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 1rem;
  }
  .close-live {
    font: inherit;
    font-size: 0.8125rem;
    font-weight: 500;
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    flex-shrink: 0;
    padding: 0.25rem 0.6rem;
    color: var(--text);
    background: var(--raised);
    border: 1px solid var(--field-line, var(--line));
    border-radius: var(--radius);
    cursor: pointer;
  }
  .close-live:hover {
    border-color: var(--gold-hover);
  }
  .close-live.danger {
    color: #fff;
    background: var(--live);
    border-color: var(--live);
  }
  .close-ask {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    flex-shrink: 0;
    font-size: 0.8125rem;
    color: var(--text);
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
  /* Never over Help and Setup: a narrower bar keeps only the name, and a
     phone-width one leaves it to Present. */
  .onair {
    overflow: hidden;
  }
  @media (max-width: 900px) {
    .onair .label,
    .onair .when {
      display: none;
    }
  }
  @media (max-width: 560px) {
    .onair {
      display: none;
    }
    header {
      gap: 0.75rem;
      padding: 0 0.75rem;
    }
    /* Help and Settings as their icons; the title still names them. */
    .setup-word {
      position: absolute;
      width: 1px;
      height: 1px;
      overflow: hidden;
      clip-path: inset(50%);
      white-space: nowrap;
    }
  }
  main {
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }
  main.arriving {
    animation: arrive 0.45s ease-out, edge 1.4s ease-out;
  }
  @keyframes arrive {
    from {
      opacity: 0;
      transform: translateY(14px);
    }
  }
  @keyframes edge {
    from {
      box-shadow: inset 0 0 0 3px var(--gold);
    }
    to {
      box-shadow: inset 0 0 0 3px transparent;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    main.arriving {
      animation: edge 1.4s ease-out;
    }
  }
  .update {
    color: var(--gold);
  }
  /* A new version is waiting: a gold pill in the footer and a dot on Setup. */
  .update-pill {
    background: var(--gold-soft);
    border: 1px solid var(--line-strong);
    color: var(--gold);
    font-weight: 600;
    padding: 0.1rem 0.6rem;
    border-radius: 999px;
  }
  .update-pill:hover {
    background: var(--gold-soft);
    color: var(--gold-hover);
  }
  .update-dot {
    width: 0.5rem;
    height: 0.5rem;
    border-radius: 50%;
    background: var(--gold);
    margin-left: 0.15rem;
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
  .stream-offline {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .stream-offline:empty {
    margin-left: -0.5rem;
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
