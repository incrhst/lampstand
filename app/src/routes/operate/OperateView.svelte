<!-- Operator panel: the running order on the left, what is on air and
     the slides of the item in view in the middle, Next and notes on the
     right (only when the item has notes). While a deck is on air the middle is its presenter view
     (current slide at its build step, next click, speaker notes, timer).
     Where OBS matters, the bar has one OBS control (the scene on air and
     the next item's scene) that opens the OBS drawer (T91); OBS's
     connection status is in the footer.
     T47 replaces this with the full operator view. -->
<script lang="ts">
  import { tick, untrack } from "svelte";
  import { request, send, type Command, type Cursor, type DisplayMatch, type OutputConfig, type Service, type ServiceSummary, type SectionView, type SlideView } from "../../lib/api";
  import { live, mainOutput, report } from "../../lib/state.svelte";
  import { sectionCursor } from "../../lib/sections";
  import { neighbourItem, nextClick, nextSlide } from "../../lib/cursor";
  import Button from "../../lib/kit/Button.svelte";
  import Dot from "../../lib/kit/Dot.svelte";
  import Icon from "../../lib/kit/Icon.svelte";
  import ListRow from "../../lib/kit/ListRow.svelte";
  import Segmented from "../../lib/kit/Segmented.svelte";
  import SlidePreview from "../../lib/kit/SlidePreview.svelte";
  import { useLiveLook } from "../../lib/liveLook.svelte";
  import StatusPill from "../../lib/kit/StatusPill.svelte";
  import { kindIcon, kindLabel, kindName } from "../../lib/kit/items";
  import PresenterView from "./PresenterView.svelte";
  import TipCard from "../../lib/kit/TipCard.svelte";
  import { nextTip, loadDismissed, saveDismissed, loadUsed, saveUsed, loadSnoozed, saveSnoozed, snooze, type Tip } from "../../lib/tips";
  import LiveLoop from "./LiveLoop.svelte";
  import SoundPanel from "./SoundPanel.svelte";
  import { musicLine } from "../../lib/audio";
  import LoopProgress from "../../lib/kit/LoopProgress.svelte";
  import ObsDrawer from "./ObsDrawer.svelte";
  import QuickAdd from "./QuickAdd.svelte";
  import { loadMediaThumb } from "../../lib/mediaThumbLoader";
  import ObsNotice from "./ObsNotice.svelte";
  import { isConnected, isScreen, obsPill, upcomingScene } from "../../lib/obs";
  import Banner from "../../lib/kit/Banner.svelte";
  import { formatWhen } from "../../lib/when";
  import { newScreen, screenOffers, type ScreenOffer } from "../../lib/screenOffers";
  import { steppedDown, windowToDemote } from "../../lib/primary";
  import { modLabel } from "../../lib/shortcuts";
  import MoreMenu from "./MoreMenu.svelte";
  import ConnectRemote from "./ConnectRemote.svelte";
  import PresentAction from "../../lib/kit/PresentAction.svelte";
  import { isPlanning } from "../../lib/presenting";
  import { cannotGoLive, isStreamDesk, screensLine, shortcutsOff } from "../../lib/roles";
  import { driverOffline, isDriving } from "../../lib/driving";

  /** This device only plans: it cannot take a service live until it presents, unless it drives the presenter. */
  let planningOnly = $derived(isPlanning(live.team) && !isDriving(live.team));
  /** Driving a presenter that is not connected: the controls wait. */
  let paused = $derived(driverOffline(live.team));

  /** Opens Setup › Screens (the no-screen prompt), Setup › OBS and Plan (the blank view); `onidle` opens where the idle screen is chosen, `ondevices` where devices are paired. */
  let { onsetup, onobs, onplan, ontips, onidle, ondevices }: { onsetup?: () => void; onobs?: () => void; onplan?: () => void; ontips?: () => void; onidle?: () => void; ondevices?: () => void } = $props();

  // A stream machine (#134) runs the stream and not the service, so it has
  // no service to open: Present is its OBS panel instead.
  let streamDesk = $derived(isStreamDesk(live.team));
  let screensNote = $derived(screensLine(live.team));

  // OBS only appears for devices that stream (lib/obs.ts).
  let pill = $derived(obsPill(live.obs));

  // The OBS drawer (T91), from the strip or with O. Closing it gives focus
  // back to the strip, so the keyboard carries on where it was.
  let obsOpen = $state(false);
  let obsToggle: HTMLButtonElement | undefined = $state();
  function setObsOpen(open: boolean) {
    obsOpen = open;
    if (!open) obsToggle?.focus({ preventScroll: true });
  }
  $effect(() => {
    if (!pill) obsOpen = false;
  });

  // With no service open, offer the next few to open: the core lists
  // upcoming ones first, then undated ones newest first.
  const OFFERED = 5;
  let services: ServiceSummary[] = $state([]);
  /** Training 0 to 2 (#190), offered below the church's own services. */
  let trainingServices: ServiceSummary[] = $state([]);
  $effect(() => {
    if (live.loaded) return;
    request({ area: "Service", request: "List" }, "Services")
      .then((all) => {
        services = all.filter((s) => !s.training).slice(0, OFFERED);
        trainingServices = all.filter((s) => s.training);
      })
      .catch(report);
  });

  async function openService(id: string) {
    try {
      live.planning = id;
      live.loaded = await request({ area: "Service", request: { Open: id } }, "Loaded");
    } catch (e) {
      report(e);
    }
  }
  /** Opens Plan on this service. */
  function planService(id: string) {
    live.planning = id;
    onplan?.();
  }
  // Screens are windows and displays. A picture sent to OBS is not one: it
  // never reaches the projector, so it does not count as "a screen is on".
  let screensOn = $derived(live.outputs.filter((o) => o.enabled && isScreen(o)).length);
  const mod = modLabel(typeof navigator === "undefined" ? "" : navigator.platform);

  // With no screen on, the prompt offers to turn one on: screens set up and
  // left off, and connected displays that are not a screen yet.
  let displays: DisplayMatch[] = $state([]);
  $effect(() => {
    if (screensOn !== 0) return;
    void live.displaysVersion; // Refetch when a monitor is plugged in or removed.
    request({ area: "Outputs", request: "ListDisplays" }, "Displays")
      .then((found) => (displays = found))
      .catch(report);
  });
  let offers = $derived(screensOn === 0 ? screenOffers(live.outputs, displays) : []);

  async function turnOn(offer: ScreenOffer) {
    const config: OutputConfig =
      offer.kind === "enable" ? { ...offer.output, enabled: true } : newScreen(offer.display, live.outputs, crypto.randomUUID());
    try {
      // A display takes the main place from the rehearsal window.
      const window = windowToDemote(live.outputs, config.target);
      if (window && config.enabled) await request({ area: "Outputs", request: { Put: steppedDown(window) } }, "Outputs");
      live.outputs = await request({ area: "Outputs", request: { Put: config } }, "Outputs");
    } catch (e) {
      report(e);
    }
  }

  async function command(c: Command) {
    if (c === "ToggleBlack") used("blank-screen");
    try {
      await send({ area: "Live", request: c });
    } catch (e) {
      report(e);
    }
  }

  function show(cursor: Cursor) {
    command({ Show: cursor });
  }

  function isLive(item: string, slide: number) {
    return live.live?.item === item && live.live?.slide === slide;
  }

  // Show / Logo / Blank is the overlay the engine reports. Black and Logo
  // toggle in the engine, so choosing one sends the toggle that gets there.
  // Choosing Show again re-shows the live slide, which brings text back
  // after Clear text.
  const modes = [
    { id: "None", label: "Show" },
    { id: "Logo", label: "Logo" },
    { id: "Black", label: "Blank" },
  ] as const;

  function setOverlay(want: "None" | "Logo" | "Black") {
    const now = live.overlay;
    if (want === now) {
      if (want === "None" && live.live) show(live.live);
      return;
    }
    if (want === "None") command(now === "Black" ? "ToggleBlack" : "ToggleLogo");
    else command(want === "Black" ? "ToggleBlack" : "ToggleLogo");
  }

  // Space or Right: Next. Left: Previous. Full stop: Black. 1 to 9 jump to
  // that verse; C, B, P, T, I, E to the next chorus, bridge, pre-chorus,
  // tag, intro or ending of the live song. Typing in a field never
  // triggers a shortcut.
  function onkeydown(e: KeyboardEvent) {
    const target = e.target as HTMLElement | null;
    if (target && (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable)) {
      return;
    }
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    if (shortcutsOff(streamDesk, !!live.loaded, !!target?.closest?.("#obs-drawer.docked"))) return;
    if (live.loaded && e.key === "/") {
      e.preventDefault();
      quickAddOpen = true;
      used("quick-add");
      return;
    }
    if (pill && live.loaded && (e.key === "o" || e.key === "O")) {
      e.preventDefault();
      setObsOpen(!obsOpen);
      return;
    }
    if (obsOpen && e.key === "Escape") {
      e.preventDefault();
      setObsOpen(false);
      return;
    }
    const keys: Record<string, Command> = {
      " ": "Next",
      ArrowRight: "Next",
      PageDown: "Next",
      ArrowLeft: "Previous",
      PageUp: "Previous",
      ".": "ToggleBlack",
    };
    const c = keys[e.key];
    if (c) {
      e.preventDefault();
      command(c);
      return;
    }
    // Shift+Down and Shift+Up choose an item in the running order: the one
    // shown in the middle pane and lit in the list. Nothing goes on air
    // until Enter starts it. Plain Up and Down are left alone, so they still
    // scroll the notes and lists and a stray press cannot change the screen.
    if (live.loaded && e.shiftKey && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
      e.preventDefault();
      const chosen = neighbourItem(items, viewed?.id ?? null, e.key === "ArrowDown" ? 1 : -1);
      if (chosen) {
        viewing = chosen;
        // Keep the chosen row in view in a long order of service.
        tick().then(() => document.querySelector(".queue .row.active")?.scrollIntoView({ block: "nearest" }));
      }
      return;
    }
    // Enter starts the chosen item from its first slide. It leaves a button
    // or link to do its own Enter, and does nothing for the item already on
    // air, so it cannot restart it by accident.
    if (live.loaded && e.key === "Enter" && !e.shiftKey && viewed && viewed.slides.length > 0 && viewed.id !== live.live?.item) {
      if (!target?.closest("button, a, select, summary, [role='button']")) {
        e.preventDefault();
        show({ item: viewed.id, slide: 0, step: 0 });
        return;
      }
    }
    if (live.loaded) {
      const cursor = sectionCursor(live.loaded, live.live, e.key);
      if (cursor) {
        e.preventDefault();
        used("jump-keys");
        show(cursor);
      }
    }
  }

  // The service behind the loaded one, for item kinds and planner notes,
  // which the live view does not carry.
  let service = $state<Service | null>(null);
  $effect(() => {
    const id = live.loaded?.service;
    if (!id) return;
    void live.loaded;
    request({ area: "Service", request: { Get: id } }, "Service")
      .then((s) => (service = s))
      .catch(report);
  });

  // The item whose slides are shown. It follows the live item; clicking
  // the running order looks at another item without taking it live.
  let viewing: string | null = $state(null);
  $effect(() => {
    const item = live.live?.item;
    if (item) viewing = item;
  });

  const isDeckSlide = (slide: SlideView) => slide.lines.length === 0 && /^Slide \d+$/.test(slide.label ?? "");

  let items = $derived(live.loaded?.items ?? []);

  // Quick add (/): a song or passage into the service on air, after what is
  // on air, without going back to Plan.
  let quickAddOpen = $state(false);
  /** The "Connect a phone or tablet" dialog (T101). */
  let connectOpen = $state(false);
  function quickAdded(id: string | null, showNow: boolean, title: string) {
    quickAddOpen = false;
    live.status = `Added ${title}`;
    if (showNow && id) show({ item: id, slide: 0, step: 0 });
  }
  let viewed = $derived(items.find((i) => i.id === viewing) ?? items.find((i) => i.slides.length > 0) ?? items[0]);
  /** "Close service" asks first when something is on air. */
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
  let liveIndex = $derived(items.findIndex((i) => i.id === live.live?.item));
  /** The section on air, if the live item sits under a header. */
  let liveSection = $derived(live.loaded?.sections.find((s) => s.items.includes(live.live?.item ?? "")) ?? null);
  /** Headers whose sections are folded away in the service order. */
  let collapsed: Set<string> = $state(new Set());
  function toggleSection(header: string) {
    const next = new Set(collapsed);
    if (!next.delete(header)) next.add(header);
    collapsed = next;
  }
  /** Tips dismissed on this device; the card offers one that applies now. */
  let dismissedTips = $state(loadDismissed());
  let usedTips = $state(loadUsed());
  /** The operator used a feature, so its tip has nothing left to teach. */
  function used(id: string) {
    if (usedTips.has(id)) return;
    usedTips = new Set([...usedTips, id]);
    saveUsed(usedTips);
  }
  function dismissTip(id: string) {
    dismissedTips = new Set([...dismissedTips, id]);
    saveDismissed(dismissedTips);
    live.tipsVersion++;
  }
  let snoozedTips = $state(loadSnoozed());
  /** "Not now": the tip comes back after its snooze, if it still applies. */
  function snoozeTip(t: Tip) {
    snoozedTips = snooze(snoozedTips, t.id, t.snoozeDays ?? 7);
    saveSnoozed(snoozedTips);
    live.tipsVersion++;
  }
  function tipAction(t: Tip) {
    if (t.action?.go === "devices") ondevices?.();
  }
  /** The order as groups: each header with the items under it. Items before
   *  the first header form a group with no header. */
  let groups = $derived.by(() => {
    type Entry = { item: (typeof items)[number]; i: number };
    const out: { section: SectionView | null; rows: Entry[] }[] = [];
    items.forEach((item, i) => {
      const section = live.loaded?.sections.find((s) => s.header === item.id) ?? null;
      if (section) out.push({ section, rows: [] });
      else {
        if (out.length === 0) out.push({ section: null, rows: [] });
        out[out.length - 1].rows.push({ item, i });
      }
    });
    return out;
  });
  /** Turn a group's loop on (at its last or the default interval) or off. */
  async function toggleLoop(section: SectionView) {
    try {
      await request(
        { area: "Service", request: { SetLiveLoop: { header: section.header, seconds: section.seconds === null ? 8 : null } } },
        "Ok",
      );
    } catch (e) {
      report(e);
    }
  }
  let liveItem = $derived(liveIndex >= 0 ? items[liveIndex] : null);
  let tip = $derived(
    nextTip(
      { loaded: live.loaded ?? null, liveItem: live.live?.item ?? null, liveSlides: liveItem?.slides.length ?? 0, hasLogo: !!live.settings?.brand?.logo, paired: live.team?.paired.length, realUse: live.realUse },
      new Set([...dismissedTips, ...usedTips]),
      undefined,
      snoozedTips,
    ),
  );
  let upcoming = $derived(live.loaded ? nextSlide(live.loaded, live.live) : null);
  // A deck on air gets the presenter view, whose Next is the next click.
  let presenting = $derived(!!(liveItem && live.live && live.liveSlide && isDeckSlide(live.liveSlide)));
  let clickNext = $derived(live.loaded ? nextClick(live.loaded, live.live) : null);
  // The picture of what is on air is large for a deck (the presenter view)
  // and for an image or video, which fills the big preview.
  let liveWidth = $derived(presenting || mediaOf(live.shown?.item) ? 1280 : 480);

  // Deck slides have no lines to draw, so the core renders a picture of
  // each at a build step, fetched once per loaded service as slides come
  // into view. Slides in the strip show every build; the live slide and
  // the next click show the step they are at.
  const THUMB_WIDTH = 480;
  const LARGE_WIDTH = 1280;
  let thumbs: Record<string, string> = $state({});
  const asked = new Set<string>();
  const thumbKey = (item: string, slide: number, step: number, width: number) => `${item}/${slide}/${step}/${width}`;

  /** The file an image or video item shows, if this is one. */
  function mediaOf(item: string | undefined) {
    const kind = item ? service?.items[item]?.kind : undefined;
    return kind && "Media" in kind ? kind.Media : null;
  }
  /** A slide with a picture to ask for: a deck's slide, or a media item's file. */
  const hasPicture = (item: string, slide: SlideView) => isDeckSlide(slide) || mediaOf(item) !== null;

  async function fetchThumb(item: string, slide: number, step: number, width = THUMB_WIDTH) {
    const key = thumbKey(item, slide, step, width);
    if (asked.has(key)) return;
    asked.add(key);
    try {
      const media = mediaOf(item);
      // An image, or a video's first frame that is not blank, from the
      // library; a deck's slide is drawn by the core at its build step.
      const picture = media
        ? await loadMediaThumb(media.blob, media.name, width)
        : await request({ area: "Service", request: { SlideThumbnail: { item, slide, step, width } } }, "Thumbnail");
      if (picture) thumbs[key] = picture;
    } catch {
      // No picture (no GPU, or the deck reloaded): the slide shows its label.
    }
  }

  const thumbFor = (item: string | undefined, slide: number, step: number, width = THUMB_WIDTH) =>
    item ? thumbs[thumbKey(item, slide, step, width)] : undefined;
  /** The strip shows a deck slide fully built. */
  const builtThumb = (item: string, slide: number) => thumbFor(item, slide, items.find((i) => i.id === item)?.slides[slide]?.steps ?? 0);

  // A reload can change a deck's slides, so pictures are fetched again.
  $effect(() => {
    void live.loaded;
    untrack(() => {
      thumbs = {};
      asked.clear();
    });
  });

  $effect(() => {
    // Most urgent first: the slide on air and what the next click shows.
    const want: [string, number, number, number][] = [];
    // While music is live the screens keep the slide before it (`shown`).
    if (live.shown && live.shownSlide && hasPicture(live.shown.item, live.shownSlide)) {
      want.push([live.shown.item, live.shown.slide, live.shown.step, liveWidth]);
    }
    if (presenting && clickNext && isDeckSlide(clickNext.slide)) {
      want.push([clickNext.cursor.item, clickNext.cursor.slide, clickNext.cursor.step, LARGE_WIDTH]);
    }
    if (upcoming && hasPicture(upcoming.cursor.item, upcoming.slide)) want.push([upcoming.cursor.item, upcoming.cursor.slide, upcoming.slide.steps, THUMB_WIDTH]);
    if (viewed) viewed.slides.forEach((s, i) => hasPicture(viewed.id, s) && want.push([viewed.id, i, s.steps, THUMB_WIDTH]));
    untrack(async () => {
      for (const [item, slide, step, width] of want) await fetchThumb(item, slide, step, width);
    });
  });
  let output = $derived(mainOutput(live.outputs));
  const look = useLiveLook(() => output);
  let showGuide = $state(true);

  // Clicker mode (T56): while a service is open, a clicker's Page Up, Page
  // Down, B (Black) and full stop work whichever window has focus. It is
  // this machine's setting, so it is read from the core, not kept here.
  let clicker = $state(false);
  $effect(() => {
    request({ area: "Settings", request: "Get" }, "Settings")
      .then((s) => (clicker = s.clicker_mode))
      .catch(report);
  });

  async function toggleClicker() {
    try {
      // Put replaces every setting, so the others are sent back as they are.
      const current = await request({ area: "Settings", request: "Get" }, "Settings");
      const s = await request({ area: "Settings", request: { Put: { ...current, clicker_mode: !clicker } } }, "Settings");
      clicker = s.clicker_mode;
    } catch (e) {
      report(e);
    }
  }

  // The logo bug follows the logo set in Setup › Church look; this flips it
  // on or off during a service and keeps the corner, size and opacity.
  let bugReady = $derived(!!live.settings?.brand?.logo);
  let bugOn = $derived(bugReady && !!live.settings?.brand?.bug.enabled);

  async function toggleBug() {
    const brand = live.settings?.brand;
    if (!brand?.logo) return;
    try {
      live.settings = await request({ area: "Settings", request: { SetLogoBug: { ...brand.bug, enabled: !brand.bug.enabled } } }, "Settings");
      used("logo-bug");
    } catch (e) {
      report(e);
    }
  }

  function kindOf(id: string) {
    const item = service?.items[id];
    return item ? kindName(item.kind) : null;
  }

  let notes = $derived(liveItem ? (service?.items[liveItem.id]?.notes ?? "") : "");

  let obsOn = $derived(isConnected(live.obs));
  let nextObsScene = $derived(
    upcomingScene(
      items.map((i) => service?.items[i.id]?.obs_scene),
      liveIndex,
      live.obs?.program_scene ?? null,
    ),
  );

  const stateLabel = { None: "LIVE", Black: "BLANK", Logo: "LOGO" } as const;
  const outputState = { None: "Showing", Black: "Blank", Logo: "Logo" } as const;
</script>

<svelte:window {onkeydown} />

{#snippet noScreen()}
  <Banner tone="warn">
    <span>No screen is on, so nothing reaches the projector.</span>
    <span class="offers">
      {#each offers as offer (offer.label)}
        <Button size="small" variant="primary" icon="monitor" onclick={() => turnOn(offer)}>{offer.label}</Button>
      {/each}
      {#if onsetup}<Button size="small" icon="monitor" onclick={onsetup}>Set up screens</Button>{/if}
    </span>
  </Banner>
{/snippet}

{#if !live.loaded && live.team === null && live.sync === null}
  <!-- Who this device is isn't known yet; the picker now would only flash. -->
  <div class="none" aria-busy="true"></div>
{:else if !live.loaded && streamDesk}
  <div class="none desk">
    <!-- The region is always there, so a change in its text is announced once. -->
    <p class="muted desk-screens" role="status">{screensNote ?? ""}</p>
    {#if pill}<p class="desk-pill"><StatusPill tone={pill.tone}>{pill.text}</StatusPill></p>{/if}
    {#if pill?.tone === "warn" || live.obs?.held || live.obs?.scene_problem}<div class="no-screen obs-notice"><ObsNotice view={live.obs} onsetup={onobs} /></div>{/if}
    <ObsDrawer docked view={live.obs} onsetup={onobs} />
  </div>
{:else if !live.loaded}
  <div class="none">
    {#if screensOn === 0}<div class="no-screen">{@render noScreen()}</div>{/if}
    {#if pill?.tone === "warn" || live.obs?.held || live.obs?.scene_problem}<div class="no-screen obs-notice"><ObsNotice view={live.obs} onsetup={onobs} /></div>{/if}
    <div class="pick">
      <div class="pick-head">
        <h1>No service is open</h1>
        <p class="muted">Open one to go live, or plan a new one.</p>
        {#if onidle}<button type="button" class="quiet-link" onclick={onidle}>Change idle screen</button>{/if}
      </div>
      {#if planningOnly}
        <div class="pick-foot gate">
          <Banner tone="neutral" icon="monitor">
            <span>{cannotGoLive(live.team?.presenter.holder_name ?? "")}</span>
            <PresentAction />
          </Banner>
        </div>
      {/if}
      {#if services.length > 0}
        <ul class="plain pick-list" aria-label="Services to open">
          {#each services as svc (svc.id)}
            <!-- An empty service has nothing to go live with: say so, and
                 offer to plan it instead. -->
            <ListRow
              title={svc.title}
              subtitle={svc.items === 0
                ? `${formatWhen(svc.starts_at, svc.time_zone)} · No items yet`
                : `${formatWhen(svc.starts_at, svc.time_zone)} · ${svc.items} ${svc.items === 1 ? "item" : "items"}`}
              divided
            >
              {#snippet trailing()}
                {#if svc.items === 0 && onplan}
                  <Button size="small" icon="calendar" onclick={() => planService(svc.id)} label="Plan {svc.title}">Plan</Button>
                {:else}
                  <Button variant="primary" size="small" icon="play" disabled={planningOnly} onclick={() => openService(svc.id)} label="Open {svc.title} for live">
                    Open
                  </Button>
                {/if}
              {/snippet}
            </ListRow>
          {/each}
        </ul>
      {/if}
      {#if trainingServices.length > 0}
        <p class="pick-group" id="pick-training">Learn by running it</p>
        <ul class="plain pick-list" aria-labelledby="pick-training">
          {#each trainingServices as svc (svc.id)}
            <ListRow title={svc.title} subtitle={`${svc.items} ${svc.items === 1 ? "item" : "items"} · works offline`} divided>
              {#snippet trailing()}
                <Button size="small" icon="play" disabled={planningOnly} onclick={() => openService(svc.id)} label="Open {svc.title} for live">
                  Open
                </Button>
              {/snippet}
            </ListRow>
          {/each}
        </ul>
      {/if}
      {#if onplan}
        <div class="pick-foot">
          <Button icon="calendar" onclick={onplan}>{services.length > 0 ? "All services in Plan" : "Go to Plan"}</Button>
        </div>
      {/if}
    </div>
  </div>
{:else}
  <div class="operate" inert={paused} class:paused>
    <div class="bar">
      <p class="service">
        <strong title={live.loaded.title}>{live.loaded.title}</strong>
        <!-- Most services share a title ("Sunday service"), so the date tells them apart. -->
        {#if service?.starts_at != null}<span class="when">· {formatWhen(service.starts_at, service.time_zone)}</span>{/if}
      </p>
      {#if confirmClose}
        <span class="close">
          <span class="close-ask">Something is on air. Close the service?</span>
          <Button size="small" variant="danger" onclick={closeService}>Close service</Button>
          <Button size="small" variant="ghost" onclick={() => (confirmClose = false)}>Keep open</Button>
        </span>
      {/if}
      {#if pill}
        <!-- The OBS controller: what OBS has on air and what the next item
             switches to. Opens the drawer of scenes, preview and mutes. The
             connection's status sits quietly in the footer. -->
        <button
          bind:this={obsToggle}
          class="obs-control"
          class:open={obsOpen}
          onclick={() => setObsOpen(!obsOpen)}
          aria-expanded={obsOpen}
          aria-controls="obs-drawer"
          title="Control OBS: switch scenes, preview them, mute audio (O)"
        >
          <span class="obs-tag"><Icon name="remote" size={14} />Control OBS<Dot tone={pill.tone} size={6} /></span>
          {#if obsOn && live.obs?.program_scene}
            <span class="on-air"><span class="sr">On air: </span>{live.obs.program_scene}</span>
            {#if nextObsScene}<span class="next">Next: {nextObsScene}</span>{/if}
          {:else}
            <span class="on-air">{obsOn ? "Scenes" : "Not connected"}</span>
          {/if}
          <span class="chevron" aria-hidden="true"><Icon name="next" size={12} /></span>
        </button>
      {/if}
      <span class="spacer"></span>
      <Segmented options={modes} value={live.overlay} onchange={setOverlay} label="Screen" />
      <MoreMenu
        guide={showGuide}
        {clicker}
        bug={bugOn}
        {bugReady}
        obs={!!pill}
        {mod}
        onguide={() => { showGuide = !showGuide; used("safe-area"); }}
        onclicker={toggleClicker}
        onbug={toggleBug}
        onclearText={() => command("ClearText")}
        onclearAll={() => command("ClearAll")}
        onconnect={() => (connectOpen = true)}
        {onidle}
      />
      <Button size="small" variant="ghost" icon="x" onclick={askClose} title="Put the service away; it stays in Plan">Close service</Button>
    </div>

    <!-- While a deck is on air the presenter view takes the right column
         too; outputs stay in the status bar. -->
    <div class="grid" class:presenting>
      {#if connectOpen}
        <ConnectRemote onclose={() => (connectOpen = false)} />
      {/if}
      {#if quickAddOpen && live.loaded}
        <QuickAdd
          service={live.loaded.service}
          order={items.map((i) => i.id)}
          liveItem={live.live?.item ?? null}
          onclose={() => (quickAddOpen = false)}
          onadded={quickAdded}
        />
      {/if}
      {#if obsOpen}
        <ObsDrawer view={live.obs} onclose={() => setObsOpen(false)} onsetup={onobs} />
      {/if}
      <nav class="pane pane-side queue" aria-label="Service order">
        <div class="pane-head">
          <h2>Service order</h2>
          <Button size="small" icon="plus" shortcut="/" onclick={() => { quickAddOpen = true; used("quick-add"); }} title="Quick add a song or scripture after what is on air">Quick add</Button>
        </div>
        {#if live.loaded && liveSection}
          <LiveLoop loaded={live.loaded} section={liveSection} />
        {/if}
        <SoundPanel {items} {service} />
        {#if tip}
          <TipCard {tip} ondismiss={() => dismissTip(tip.id)} onall={() => ontips?.()} onaction={() => tipAction(tip)} onsnooze={() => snoozeTip(tip)} />
        {/if}
        <div class="pane-body groups">
          {#each groups as group (group.section?.header ?? "ungrouped")}
            {@const section = group.section}
            <section class="group" class:boxed={!!section} class:looping={!!section?.seconds} aria-label={section?.title}>
              {#if section}
                <div class="eyebrow-row">
                  <button
                    type="button"
                    class="eyebrow fold"
                    class:folded={collapsed.has(section.header)}
                    aria-expanded={!collapsed.has(section.header)}
                    title={collapsed.has(section.header) ? "Expand" : "Collapse"}
                    onclick={() => toggleSection(section.header)}
                  >
                    <Icon name="chevronDown" size={14} />{section.title}
                  </button>
                  <button
                    type="button"
                    class="loop-toggle"
                    class:on={section.seconds !== null}
                    aria-pressed={section.seconds !== null}
                    title={section.seconds === null ? "Loop this group" : "Stop looping this group"}
                    onclick={() => toggleLoop(section)}
                  >
                    <Icon name="loop" size={14} />{section.seconds === null ? "Loop" : `Looping · ${section.seconds} s`}
                  </button>
                </div>
              {/if}
              {#if !section || !collapsed.has(section.header)}
                <ul class="plain">
                  {#each group.rows as { item, i } (item.id)}
                    {@const kind = kindOf(item.id)}
                    <ListRow
                      title={item.title}
                      subtitle={item.slides.length === 0 ? (kind === "Sound" ? musicLine(item) : "Nothing to show") : `${item.slides.length} ${item.slides.length === 1 ? "slide" : "slides"}`}
                      accent={i === liveIndex}
                      active={item.id === viewed?.id && i !== liveIndex}
                      dim={liveIndex >= 0 && i < liveIndex}
                      onclick={() => (viewing = item.id)}
                    >
                      {#snippet leading()}
                        <span class="num index" class:current={i === liveIndex}>{i + 1}</span>
                        <span class:current={i === liveIndex}>
                          <Icon name={kind ? kindIcon[kind] : "file"} label={kind ? kindLabel[kind] : undefined} />
                        </span>
                      {/snippet}
                    </ListRow>
                  {/each}
                </ul>
              {:else if liveSection?.header === section.header}
                <p class="muted folded-note">Folded, with the live slide inside.</p>
              {/if}
            </section>
          {/each}
        </div>
      </nav>

      <section class="pane centre" aria-label="Live screen">
        <div class="centre-body">
          {#if screensOn === 0}<div class="no-screen">{@render noScreen()}</div>{/if}
          {#if pill?.tone === "warn" || live.obs?.held || live.obs?.scene_problem}<div class="no-screen obs-notice"><ObsNotice view={live.obs} onsetup={onobs} /></div>{/if}
          {#if presenting && liveItem && live.live}
            {#key liveItem.id}
              <PresenterView
                item={liveItem}
                cursor={live.live}
                upcoming={clickNext}
                current={thumbFor(liveItem.id, live.live.slide, live.live.step, LARGE_WIDTH)}
                nextImage={clickNext ? thumbFor(clickNext.cursor.item, clickNext.cursor.slide, clickNext.cursor.step, LARGE_WIDTH) : undefined}
                overlay={live.overlay}
                safeArea={output?.safe_area}
                {showGuide}
                plannerNotes={notes}
                onprevious={() => command("Previous")}
                onnext={() => command("Next")}
                onblank={() => command("ToggleBlack")}
              />
            {/key}
          {:else}
            <div class="live-head">
              <span class="state"><Dot tone={live.overlay === "None" && live.live ? "live" : "neutral"} size={8} />{stateLabel[live.overlay]}</span>
              <span class="muted">{output?.name ?? "No screen yet"}</span>
              {#if liveItem && live.live}
                <span class="muted">· {liveItem.title} · {liveItem.slides.length === 0 ? "Music only" : (live.liveSlide?.label ?? live.live.slide + 1)}</span>
            {/if}
          </div>
          <div class="live-screen" class:on={live.overlay === "None" && !!live.live} aria-live="polite">
            <SlidePreview
              lines={live.shownSlide?.lines ?? []}
              footer={live.shownSlide?.footer ?? null}
              image={live.shown ? thumbFor(live.shown.item, live.shown.slide, live.shown.step, liveWidth) : null}
              overlay={live.overlay}
              safeArea={output?.safe_area}
              {showGuide}
              size="large"
              idle={live.idle ? look.idle : null}
              brand={look.brand}
              empty="Nothing on air. Press Space to start."
            />
            {#if live.live && live.overlay === "None" && liveSection?.seconds}
              <LoopProgress seconds={liveSection.seconds} slideKey="{live.live.item}:{live.live.slide}:{live.live.step}" />
            {/if}
          </div>
          {#if live.idle && live.overlay === "None" && onidle}
            <button type="button" class="quiet-link idle-link" onclick={onidle}>Change idle screen</button>
          {/if}
          <div class="transport">
            <Button icon="prev" onclick={() => command("Previous")} shortcut="←">Previous</Button>
            <Button variant="primary" onclick={() => command("Next")} shortcut="Space">Next</Button>
            <Button variant="danger" onclick={() => command("ToggleBlack")} pressed={live.overlay === "Black"} shortcut=".">Blank</Button>
          </div>
          {/if}

          {#if viewed}
            <div class="slides-head">
              <h2 class="title">{viewed.title}</h2>
            </div>
            {#if viewed.slides.length === 0}
              <p class="muted nothing">Nothing to show.</p>
            {/if}
            <ul class="plain slides">
              {#each viewed.slides as slide, i}
                <li>
                  <button
                    class="slide"
                    class:live={isLive(viewed.id, i)}
                    onclick={() => show({ item: viewed.id, slide: i, step: 0 })}
                    aria-current={isLive(viewed.id, i) ? "true" : undefined}
                  >
                    <SlidePreview lines={slide.lines} footer={slide.footer} image={builtThumb(viewed.id, i)} size="small" />
                    <span class="caption">
                      {#if isLive(viewed.id, i)}<StatusPill tone="live">LIVE</StatusPill>{/if}
                      <span>{slide.label ?? i + 1}</span>
                      {#if slide.steps > 0}<span class="faint steps">{slide.steps} {slide.steps === 1 ? "step" : "steps"}</span>{/if}
                    </span>
                  </button>
                </li>
              {/each}
            </ul>
          {/if}
        </div>
      </section>

      {#if !presenting}
        <aside class="pane pane-side side" aria-label="Next and notes">
          <section>
            <div class="side-head">
              <h2 class="title">Next</h2>
              {#if upcoming}<span class="muted">{upcoming.title} · {upcoming.slide.label ?? upcoming.cursor.slide + 1}</span>{/if}
            </div>
            <div class="next">
              {#if upcoming}
                <SlidePreview
                  lines={upcoming.slide.lines}
                  footer={upcoming.slide.footer}
                  image={builtThumb(upcoming.cursor.item, upcoming.cursor.slide)}
                  safeArea={output?.safe_area}
                />
              {:else}
                <SlidePreview empty="End of service" />
              {/if}
            </div>
          </section>

          {#if notes}
            <section>
              <h2 class="title">Notes</h2>
              <p class="notes">{notes}</p>
            </section>
          {/if}
        </aside>
      {/if}
    </div>
  </div>
{/if}

<style>
  .obs-notice {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  /* A clear control, not a status label: framed, with an OBS tag, the
     scene on air and a chevron. "Next: …" gives way first when the bar is
     short of room, then the scene's name. */
  .obs-control {
    font: inherit;
    font-size: 0.8125rem;
    color: var(--text);
    display: inline-flex;
    align-items: center;
    gap: 0.5rem;
    min-width: 9rem;
    max-width: 24rem;
    height: 2.1rem;
    padding: 0 0.6rem 0 0.25rem;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius);
    background: var(--raised);
    cursor: pointer;
    white-space: nowrap;
  }
  .obs-control:hover {
    border-color: var(--muted);
  }
  .obs-control.open {
    border-color: var(--gold);
  }
  .obs-tag {
    flex-shrink: 0;
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    height: 1.55rem;
    padding: 0 0.45rem;
    border-radius: 4px;
    background: var(--panel);
    font-size: 0.7rem;
    font-weight: 700;
    letter-spacing: 0.06em;
  }
  .obs-control > span:not(.obs-tag):not(.chevron) {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .obs-control > .on-air {
    min-width: 3.5rem;
    flex: 0 1 auto;
    font-weight: 500;
  }
  .obs-control > .next {
    flex: 0 100 auto;
    color: var(--muted);
  }
  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
  }
  .chevron {
    flex-shrink: 0;
    display: inline-flex;
    color: var(--muted);
    transition: transform 120ms var(--ease);
  }
  .obs-control.open .chevron {
    transform: rotate(90deg);
  }
  .none {
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.75rem;
    color: var(--faint);
    font-size: 0.875rem;
  }
  .operate {
    height: 100%;
    display: flex;
    flex-direction: column;
  }
  .operate.paused {
    opacity: 0.55;
  }
  .bar > :global(*) {
    flex-shrink: 0;
  }
  .bar {
    flex-shrink: 0;
    display: flex;
    flex-wrap: nowrap;
    align-items: center;
    gap: 0.5rem 1rem;
    padding: 0.6rem 1.25rem;
    border-bottom: 1px solid var(--line);
  }
  /* At tablet width the controls don't fit on one row: they wrap rather
     than squeeze the OBS strip or the screen buttons to nothing. */
  @media (max-width: 900px) {
    .bar {
      flex-wrap: wrap;
    }
  }
  /* A long title truncates and the date keeps its width, so the controls
     stay on one row in the same place whatever the service is called. */
  .close {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
  }
  .close-ask {
    font-size: 0.8125rem;
    color: var(--muted);
  }
  .service {
    display: flex;
    gap: 0.3rem;
    min-width: 6rem;
    max-width: 28rem;
    font-size: 0.9rem;
    white-space: nowrap;
  }
  .service strong {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    font-weight: 500;
  }
  .when {
    flex-shrink: 0;
    color: var(--muted);
  }
  .spacer {
    flex: 1 1 0;
  }
  .grid {
    position: relative;
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: 250px minmax(0, 1fr) 320px;
  }
  .grid.presenting {
    grid-template-columns: 250px minmax(0, 1fr);
  }
  .queue {
    border-right: 1px solid var(--line);
  }
  .index {
    width: 1rem;
    text-align: right;
    font-size: 0.75rem;
    color: var(--faint);
  }
  .current {
    color: var(--gold);
  }

  .no-screen {
    margin-bottom: 1rem;
  }
  .desk {
    justify-content: flex-start;
    gap: 1rem;
    padding: 1.5rem 1.25rem;
    overflow-y: auto;
  }
  .desk-pill {
    margin: 0 auto;
    width: min(640px, 100%);
  }
  .desk-screens {
    margin: 0 auto;
    width: min(640px, 100%);
    font-size: 0.875rem;
  }
  .pick {
    width: min(32rem, calc(100% - 2rem));
    color: var(--text);
  }
  .pick-head {
    text-align: center;
    margin-bottom: 1rem;
  }
  .pick-head h1 {
    font-size: 1.1rem;
    font-weight: 600;
  }
  .pick-head p {
    margin-top: 0.25rem;
  }
  .pick-list {
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    background: var(--panel);
  }
  .pick-group {
    margin: 1.25rem 0 0.4rem;
    font-size: 0.75rem;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--muted);
  }
  .pick-foot {
    display: flex;
    justify-content: center;
    margin-top: 1rem;
  }
  .none .no-screen {
    width: min(36rem, calc(100% - 2rem));
    color: var(--text);
  }
  .offers {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    justify-content: flex-end;
  }
  .no-screen :global(.body) {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.75rem;
  }
  .centre-body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 1.25rem 1.5rem 2rem;
  }
  .live-head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.75rem;
    margin-bottom: 0.5rem;
  }
  .state {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    font-weight: 700;
    letter-spacing: 0.04em;
  }
  .groups {
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
    padding: 0 0.5rem 0.5rem;
  }
  .group.boxed {
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-lg);
    padding: 0.25rem 0.25rem 0.35rem;
  }
  .group.looping {
    border-color: var(--text);
  }
  .eyebrow-row {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.35rem;
    padding: 0.2rem 0.25rem 0.35rem 0.5rem;
  }
  .eyebrow {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    min-width: 0;
    font: inherit;
    font-size: 0.7rem;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--muted);
    background: none;
    border: none;
    padding: 0.2rem 0;
    cursor: pointer;
  }
  .eyebrow :global(.icon) {
    transition: transform 150ms var(--ease);
  }
  .eyebrow.folded :global(.icon) {
    transform: rotate(-90deg);
  }
  .loop-toggle {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    flex-shrink: 0;
    font: inherit;
    font-size: 0.75rem;
    padding: 0.25rem 0.6rem;
    border-radius: var(--radius);
    border: 1px solid var(--line-strong);
    background: var(--raised);
    color: var(--text);
    cursor: pointer;
  }
  .loop-toggle.on {
    background: var(--gold);
    border-color: var(--gold);
    color: var(--on-gold);
    font-weight: 600;
  }
  .folded-note {
    margin: 0;
    padding: 0.25rem 0.75rem;
    font-size: 0.75rem;
  }
  .live-screen {
    position: relative;
    border-radius: var(--radius-lg);
    overflow: hidden;
    box-shadow: 0 0 0 2px var(--line);
  }
  .live-screen.on {
    box-shadow: 0 0 0 2px var(--live);
  }
  .quiet-link {
    font: inherit;
    font-size: 0.8rem;
    color: var(--muted);
    background: none;
    border: none;
    padding: 0;
    text-decoration: underline;
    cursor: pointer;
  }
  .quiet-link:hover {
    color: var(--text);
  }
  .idle-link {
    display: block;
    margin-top: 0.4rem;
  }
  .pick-head .quiet-link {
    margin-top: 0.35rem;
  }
  .transport {
    display: flex;
    gap: 0.5rem;
    margin-top: 0.9rem;
  }
  .slides-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    margin-top: 2rem;
    font-size: 0.75rem;
  }
  .nothing {
    margin-top: 0.75rem;
    font-size: 0.875rem;
  }
  .slides {
    margin-top: 0.75rem;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
    gap: 0.75rem;
  }
  .slide {
    font: inherit;
    color: inherit;
    width: 100%;
    padding: 0;
    text-align: left;
    background: var(--panel);
    border: none;
    border-radius: var(--radius);
    overflow: hidden;
    cursor: pointer;
    box-shadow: 0 0 0 1px var(--line);
  }
  .slide:hover {
    box-shadow: 0 0 0 1px var(--line-strong);
  }
  .slide.live {
    box-shadow: 0 0 0 2px var(--live);
  }
  .caption {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.35rem 0.6rem;
    font-size: 0.75rem;
    color: var(--muted);
  }
  .slide.live .caption {
    color: var(--text);
    font-weight: 500;
  }
  .caption :global(.pill) {
    padding: 0.05rem 0.4rem;
    font-size: 0.62rem;
  }
  .steps {
    margin-left: auto;
  }

  .side {
    border-left: 1px solid var(--line);
    overflow-y: auto;
    padding: 1.25rem;
    gap: 1.75rem;
  }
  .side-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.5rem;
    font-size: 0.75rem;
  }
  .side-head span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .next {
    margin-top: 0.5rem;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    overflow: hidden;
  }
  .notes {
    margin-top: 0.5rem;
    font-size: 0.95rem;
    line-height: 1.6;
    white-space: pre-wrap;
  }
</style>
