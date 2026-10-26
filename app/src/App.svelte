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
  import ModeSwitch from "./lib/kit/ModeSwitch.svelte";

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
      return;
    }
    // F: Focus in Plan and Present. Esc leaves it in Plan; Present's own
    // Esc closes its OBS drawer first. Typing in a field never triggers it.
    const target = e.target as HTMLElement | null;
    if (target && (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.tagName === "SELECT" || target.isContentEditable)) return;
    if (e.metaKey || e.ctrlKey || e.altKey || !focusable) return;
    if (e.key === "f" || e.key === "F") {
      e.preventDefault();
      live.focus = !live.focus;
    } else if (e.key === "Escape" && focused && screen === "plan") {
      e.preventDefault();
      live.focus = false;
    }
  }

  // Focus is one switch for Plan and Present: Plan keeps the order of
  // service, Present the cards. Switching modes keeps it; Present needs a
  // service open to have anything to focus on.
  let focusable = $derived(screen === "plan" || (screen === "operate" && !!live.loaded));
  let focused = $derived(live.focus && focusable);

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
  <!-- Focus hides the bar; its pill (here for Plan, on the wall for Present) keeps the way out and the other mode. -->
  {#if !focused}
  <header>
    <nav aria-label="Screens">
      {#each screens as s}
        <button class:active={screen === s.id} aria-current={screen === s.id ? "page" : undefined} title="{s.label} ({mod}{s.key}; {mod}\ switches between Plan and Present)" onclick={() => (screen = s.id)}>
          {s.label}
        </button>
      {/each}
    </nav>
    <div class="right">
      {#if live.loaded}
        <p class="onair">
          <span class="label">Live service:</span>
          <strong title={live.loaded.title}>{live.loaded.title}</strong>
          {#if liveWhen}<span class="when">· {liveWhen}</span>{/if}
        </p>
      {/if}
      <button class="setup" class:active={screen === "help"} aria-current={screen === "help" ? "page" : undefined} title="Help" onclick={() => (screen = "help")}>
        <Icon name="info" size={16} />
        <span class="setup-word">Help</span>
      </button>
      <button class="setup" class:active={screen === "setup"} aria-current={screen === "setup" ? "page" : undefined} title="Setup" onclick={() => (screen = "setup")}>
        <Icon name="setup" size={16} />
        <span class="setup-word">Setup</span>
      </button>
      {#if focusable}
        <button class="setup focus-on" title="{screen === "plan" ? "Just the order of service: hide the services and the Library" : "Cards only: hide the bar and the sidecard"} (F)" onclick={() => (live.focus = true)}>
          <Icon name="focus" size={16} />
          <span class="setup-word">Focus</span>
          <kbd>F</kbd>
        </button>
      {/if}
    </div>
  </header>
  {:else if screen === "plan"}
  <div class="focus-row">
    <div class="focus-pill" role="group" aria-label="Focus">
      <ModeSwitch current="plan" {mod} onpresent={() => (screen = "operate")} />
      <button class="focus-exit" title="Show the services and the Library again (F or Esc)" onclick={() => (live.focus = false)}>Exit focus <kbd>F</kbd></button>
    </div>
  </div>
  {/if}

  <PresenterNotices />

  {#if showStrip}<GetStarted steps={started} onstep={startStep} ontell={told ? undefined : tellUs} onhide={hide} />{/if}

  <main class:arriving>
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
        {#if screenOutputs.length > 0}<span>· {enabled} on{planningOnly && enabled > 0 ? ", dark here (this device does not run the screens)" : ""}</span>{/if}
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
  /* Framed, so the way into Focus is easy to find; its pill takes this corner in Focus. */
  .focus-on {
    border: 1px solid var(--line-strong);
    color: var(--text);
  }
  .focus-on:hover {
    border-color: var(--muted);
  }
  .focus-on kbd,
  .focus-exit kbd {
    font: inherit;
    font-size: 0.72em;
    opacity: 0.7;
    padding: 0 0.3rem;
    border: 1px solid currentColor;
    border-radius: 3px;
  }
  .focus-row {
    flex-shrink: 0;
    display: flex;
    justify-content: flex-end;
    padding: 0.5rem 1rem 0;
  }
  .focus-pill {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.25rem;
    border: 1px solid var(--line-strong);
    border-radius: 999px;
    background: var(--panel);
    font-size: 0.8rem;
  }
  .focus-exit {
    font: inherit;
    color: var(--text);
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    min-height: 1.75rem;
    padding: 0 0.6rem 0 0.75rem;
    border: 1px solid var(--line-strong);
    border-radius: 999px;
    background: none;
    cursor: pointer;
  }
  .focus-exit:hover {
    border-color: var(--muted);
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
    /* Help and Setup as their icons; the title still names them. */
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
