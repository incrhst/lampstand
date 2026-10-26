<!-- Operator panel: the wall. Every item's slides in the order of
     service, the slide on air marked LIVE and the one Next shows marked
     NEXT. The sidecard beside it holds what is on air with Previous, Next
     and Blank, the screen, the room (OBS, Quick add, Sound, the group loop),
     and cards that fold away: remotes, clearing and the keys. While a deck
     slide with builds is on air it also shows the next click. Where OBS
     matters, one OBS control (the scene on air and the next item's scene)
     opens the OBS drawer (T91); OBS's connection status is in the footer. -->
<script lang="ts">
  import { tick, untrack } from "svelte";
  import { request, send, type Command, type Cursor, type DisplayMatch, type OutputConfig, type Service, type ServiceSummary, type SectionView, type SlideView } from "../../lib/api";
  import { live, mainOutput, report, sound } from "../../lib/state.svelte";
  import { sectionCursor, sectionTone } from "../../lib/sections";
  import SectionTag from "../../lib/kit/SectionTag.svelte";
  import { neighbourItem, nextClick, nextSlide } from "../../lib/cursor";
  import Button from "../../lib/kit/Button.svelte";
  import Dot from "../../lib/kit/Dot.svelte";
  import Icon from "../../lib/kit/Icon.svelte";
  import ListRow from "../../lib/kit/ListRow.svelte";
  import Segmented from "../../lib/kit/Segmented.svelte";
  import SlidePreview from "../../lib/kit/SlidePreview.svelte";
  import StatusPill from "../../lib/kit/StatusPill.svelte";
  import { kindIcon, kindLabel, kindName } from "../../lib/kit/items";
  import TipCard from "../../lib/kit/TipCard.svelte";
  import { nextTip, loadDismissed, saveDismissed, loadUsed, saveUsed, loadSnoozed, saveSnoozed, snooze, type Tip } from "../../lib/tips";
  import LiveLoop from "./LiveLoop.svelte";
  import SoundPanel from "./SoundPanel.svelte";
  import { musicLine, words } from "../../lib/audio";
  import ObsDrawer from "./ObsDrawer.svelte";
  import QuickAdd from "./QuickAdd.svelte";
  import { loadMediaThumb } from "../../lib/mediaThumbLoader";
  import ObsNotice from "./ObsNotice.svelte";
  import { isConnected, isScreen, obsPill, upcomingScene } from "../../lib/obs";
  import Banner from "../../lib/kit/Banner.svelte";
  import { formatWhen } from "../../lib/when";
  import { newScreen, screenOffers, type ScreenOffer } from "../../lib/screenOffers";
  import { steppedDown, windowToDemote } from "../../lib/primary";
  import { isTyping, KEYS as keys, modLabel } from "../../lib/shortcuts";
  import ModeSwitch from "../../lib/kit/ModeSwitch.svelte";
  import SideCard from "../../lib/kit/SideCard.svelte";
  import Switch from "../../lib/kit/Switch.svelte";
  import ConnectRemote from "./ConnectRemote.svelte";
  import PresentAction from "../../lib/kit/PresentAction.svelte";
  import { isPlanning } from "../../lib/presenting";
  import { cannotGoLive, isStreamDesk, screensLine, shortcutsOff } from "../../lib/roles";
  import { driverOffline, isDriving } from "../../lib/driving";
  import { slideMark } from "../../lib/presentView";
  import { useLiveLook } from "../../lib/liveLook.svelte";
  import { trainingVideosBusy, trainingVideosLine } from "../../lib/trainingVideos";

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

  // The OBS drawer (T91), from the OBS control or with O. Closing it gives focus
  // back to the control, so the keyboard carries on where it was.
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
  /** Training Sessions 0 to 2 (#190), offered below the church's own services. */
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

  // The training videos download the first time a training service is
  // opened (#190); until then, and offline, Present says so.
  $effect(() => {
    if (live.trainingVideos !== null || trainingServices.length === 0) return;
    request({ area: "Service", request: "TrainingVideos" }, "TrainingVideos")
      .then((v) => (live.trainingVideos = v))
      .catch(report);
  });
  let trainingLine = $derived(trainingVideosLine(live.trainingVideos));
  let loadedTraining = $derived(trainingServices.some((s) => s.id === live.loaded?.service));

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

  // Show / Idle / Blank is the overlay the engine reports. Idle puts the
  // idle screen up without ending the service; Black and Idle toggle in the
  // engine, so choosing one sends the toggle that gets there. Choosing Show
  // again re-shows the live slide, which brings text back after Clear text.
  // The church's logo is the logo bug (the Screen card), not a screen of its own.
  const modes = [
    { id: "None", label: "Show" },
    { id: "Idle", label: "Idle" },
    { id: "Black", label: "Blank" },
  ] as const;

  function setOverlay(want: "None" | "Idle" | "Black") {
    const now = live.overlay;
    if (want === now) {
      if (want === "None" && live.live) show(live.live);
      return;
    }
    if (want === "None") command(now === "Black" ? "ToggleBlack" : "ToggleIdle");
    else command(want === "Black" ? "ToggleBlack" : "ToggleIdle");
  }

  // Space or Right: Next. Left: Previous. Full stop: Black. 1 to 9 jump to
  // that verse; C, B, P, T, I, E to the next chorus, bridge, pre-chorus,
  // tag, intro or ending of the live song. Typing in a field never
  // triggers a shortcut. A switch (Logo bug, Safe area guide) is not a
  // field: Space stays Next there, and does not flip the switch.
  function onkeydown(e: KeyboardEvent) {
    const target = e.target as HTMLElement | null;
    if (isTyping(target)) return;
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
    // F (Focus) is the menu bar's, in App.svelte. Esc leaves it here.
    if (live.focus && e.key === "Escape") {
      e.preventDefault();
      setFocus(false);
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
        tick().then(() => document.querySelector(".wall-row.viewed")?.scrollIntoView({ block: "nearest" }));
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
  // The row on air scrolls into view when the service moves to
  // another item, so Next never leaves the operator looking elsewhere.
  $effect(() => {
    const item = live.live?.item;
    if (!item) return;
    tick().then(() => document.querySelector(".wall-row.on-air")?.scrollIntoView({ block: "nearest", behavior: "smooth" }));
  });
  // A deck on air gets the presenter view, whose Next is the next click.
  let presenting = $derived(!!(liveItem && live.live && live.liveSlide && isDeckSlide(live.liveSlide)));
  let clickNext = $derived(live.loaded ? nextClick(live.loaded, live.live) : null);
  /** A deck slide with builds is on air: the one time the wall shows the screen and the next click, because the cards show each slide fully built. */
  let stepped = $derived(!!live.live && (live.liveSlide?.steps ?? 0) > 0);
  /** Focus: the cards only. F turns it on and off, Esc leaves it; closing the service leaves it too. */
  function setFocus(on: boolean) {
    live.focus = on;
  }
  $effect(() => {
    if (!live.loaded) live.focus = false;
  });
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
    // The wall shows every item's slides, so every picture is wanted, after
    // the ones above.
    for (const item of items) item.slides.forEach((s, i) => hasPicture(item.id, s) && want.push([item.id, i, s.steps, THUMB_WIDTH]));
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

  const stateLabel = { None: "LIVE", Black: "BLANK", Idle: "IDLE" } as const;
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

<!-- One slide of an item, marked when it is on air or what Next shows. -->
{#snippet slideCard(itemId: string, slide: SlideView, i: number, mark: "live" | "next" | null)}
  <button
    class="slide {sectionTone(slide.label) ?? ''}"
    class:live={mark === "live"}
    class:next={mark === "next"}
    onclick={() => show({ item: itemId, slide: i, step: 0 })}
    aria-current={mark === "live" ? "true" : undefined}
  >
    <SlidePreview lines={slide.lines} footer={slide.footer} image={builtThumb(itemId, i)} size="small" />
    <span class="caption">
      {#if mark === "live"}<StatusPill tone="live">LIVE</StatusPill>{:else if mark === "next"}<StatusPill tone="gold">NEXT</StatusPill>{/if}
      <SectionTag label={slide.label ?? String(i + 1)} />
      {#if slide.steps > 0}<span class="faint steps">{slide.steps} {slide.steps === 1 ? "step" : "steps"}</span>{/if}
    </span>
  </button>
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
            <ListRow title={svc.title} subtitle={`${svc.items} ${svc.items === 1 ? "item" : "items"}`} divided>
              {#snippet trailing()}
                <Button size="small" icon="play" disabled={planningOnly} onclick={() => openService(svc.id)} label="Open {svc.title} for live">
                  Open
                </Button>
              {/snippet}
            </ListRow>
          {/each}
        </ul>
        <!-- The videos are not in the installer; the rest runs offline. -->
        <p class="pick-note muted" role="status">{trainingLine ?? ""}</p>
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
    <div class="grid">
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
      <!-- The wall: every item's slides at once, in the order of service,
           so any slide is a click away. The LIVE and NEXT marks on the
           cards say what is on air and what Next shows. The sidecard beside
           it holds the screen, the controls and the room, as cards that
           fold; Focus (F) hides it and the menu bar. -->
      <section class="pane wall-pane" aria-label="The whole service">
        {#if live.focus}
          <div class="focus-pill" role="group" aria-label="Focus">
            <ModeSwitch current="operate" {mod} {onplan} />
            <span class="state" role="status"><Dot tone={live.overlay === "None" && live.live ? "live" : "neutral"} size={8} />{stateLabel[live.overlay]}</span>
            {#if liveItem && live.live}<span class="muted focus-where">{liveItem.title} · {liveItem.slides.length === 0 ? "Music only" : (live.liveSlide?.label ?? live.live.slide + 1)}</span>{/if}
            <!-- Labelled controls, so Focus never relies on keys alone; the
                 panic controls stay within reach. -->
            <Button size="small" icon="prev" onclick={() => command("Previous")} title="Previous (←)">Previous</Button>
            <Button size="small" variant="primary" onclick={() => command("Next")} shortcut="Space">Next</Button>
            <Button size="small" variant="danger" onclick={() => command("ToggleBlack")} pressed={live.overlay === "Black"} shortcut=".">Blank</Button>
            <Button size="small" variant="danger" onclick={() => sound("Panic").catch(report)}>{words.panic}</Button>
            <Button size="small" shortcut="F" onclick={() => setFocus(false)} title="Show the menu bar and the sidecard again (F or Esc)">Exit focus</Button>
          </div>
        {/if}
        <div class="wall-body">
          {#each groups as group (group.section?.header ?? "ungrouped")}
            {@const section = group.section}
            <section class="wall-group" class:looping={!!section?.seconds} aria-label={section?.title}>
              {#if section}
                <div class="eyebrow-row wall-eyebrow">
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
                {#each group.rows as { item, i } (item.id)}
                  {@const kind = kindOf(item.id)}
                  <section
                    class="wall-row"
                    class:on-air={i === liveIndex}
                    class:played={liveIndex >= 0 && i < liveIndex}
                    class:viewed={item.id === viewed?.id && i !== liveIndex}
                    aria-label={item.title}
                  >
                    <div class="wall-label">
                      <span class="num index" class:current={i === liveIndex}>{i + 1}</span>
                      <span class:current={i === liveIndex}>
                        <Icon name={kind ? kindIcon[kind] : "file"} label={kind ? kindLabel[kind] : undefined} />
                      </span>
                      <div class="wall-title">
                        <h3>{item.title}</h3>
                        <p class="muted">
                          {item.slides.length === 0 ? (kind === "Sound" ? musicLine(item) : "Nothing to show") : `${item.slides.length} ${item.slides.length === 1 ? "slide" : "slides"}`}
                        </p>
                        {#if i === liveIndex}
                          <p class="on-air-tag"><Dot tone="live" size={6} />On air</p>
                          {#if presenting && live.liveSlide?.notes}<p class="wall-notes"><span class="faint">Speaker notes</span>{live.liveSlide.notes}</p>{/if}
                          {#if notes}<p class="wall-notes"><span class="faint">Notes</span>{notes}</p>{/if}
                        {/if}
                      </div>
                    </div>
                    {#if item.slides.length > 0}
                      <ul class="plain slides wall-slides">
                        {#each item.slides as slide, s}
                          <li>{@render slideCard(item.id, slide, s, slideMark(item.id, s, live.live, upcoming?.cursor))}</li>
                        {/each}
                      </ul>
                    {/if}
                  </section>
                {/each}
              {:else if liveSection?.header === section.header}
                <p class="muted folded-note">Folded, with the live slide inside.</p>
              {/if}
            </section>
          {/each}
        </div>
        {#if !live.focus}
        <!-- Wide, one column: the notices, On air, Sound, then the rest.
             Narrow, On air stands on its own beside a column of the rest
             (Sound first) that scrolls, so Previous, Next, Blank and Stop
             the sound now always stay in view. -->
        <aside class="sidecard" aria-label="On air and the room">
          <div class="side-onair">
            <SideCard title="On air">
              <figure class="monitor">
                <div class="monitor-screen" class:on={live.overlay === "None" && !!live.live} role="img" aria-label="What {output?.name ?? 'the main screen'} shows">
                  <SlidePreview
                    lines={live.shownSlide?.lines ?? []}
                    footer={live.shownSlide?.footer ?? null}
                    image={live.shown ? thumbFor(live.shown.item, live.shown.slide, live.shown.step, liveWidth) : null}
                    overlay={live.overlay}
                    safeArea={output?.safe_area}
                    {showGuide}
                    size="small"
                    idle={live.idle || live.overlay === "Idle" ? look.idle : null}
                    brand={look.brand}
                    empty="Nothing on air"
                  />
                </div>
              </figure>
              <div class="wall-now">
                <span class="state"><Dot tone={live.overlay === "None" && live.live ? "live" : "neutral"} size={8} />{stateLabel[live.overlay]}</span>
                <span class="muted wall-where">
                  {#if live.overlay === "Idle" && liveItem && live.live}
                    {"Idle screen · Show returns to "}{liveItem.title} · {live.liveSlide?.label ?? live.live.slide + 1}{#if upcoming}{" · Next shows "}{upcoming.title} · {upcoming.slide.label ?? upcoming.cursor.slide + 1}{/if}
                  {:else if liveItem && live.live}{liveItem.title} · {liveItem.slides.length === 0 ? "Music only" : (live.liveSlide?.label ?? live.live.slide + 1)}{:else}Nothing on air. Press Space to start.{/if}
                </span>
              </div>
              <div class="transport">
                <Button icon="prev" onclick={() => command("Previous")} title="Previous (←)">Previous</Button>
                <Button variant="primary" onclick={() => command("Next")} shortcut="Space">Next</Button>
                <Button variant="danger" onclick={() => command("ToggleBlack")} pressed={live.overlay === "Black"} shortcut=".">Blank</Button>
              </div>
              {#if stepped && live.overlay !== "Idle" && liveItem && live.live && live.liveSlide}
                {@const left = live.liveSlide.steps - live.live.step}
                <section class="wall-steps" aria-label="Builds on this slide">
                  <p class="wall-steps-head muted">{left === 0 ? "Fully built. Next goes to the next slide." : `${left} ${left === 1 ? "click" : "clicks"} left on this slide`}</p>
                  <figure>
                    {#if clickNext}
                      <SlidePreview lines={clickNext.slide.lines} footer={clickNext.slide.footer} image={thumbFor(clickNext.cursor.item, clickNext.cursor.slide, clickNext.cursor.step, LARGE_WIDTH)} size="small" />
                    {:else}
                      <SlidePreview empty="End of service" size="small" />
                    {/if}
                    <figcaption class="muted">Next click</figcaption>
                  </figure>
                </section>
              {/if}
            </SideCard>
          </div>

          <div class="side-rest">
            {#if screensOn === 0}<div class="no-screen side-notice">{@render noScreen()}</div>{/if}
            {#if pill?.tone === "warn" || live.obs?.held || live.obs?.scene_problem}<div class="no-screen obs-notice side-notice"><ObsNotice view={live.obs} onsetup={onobs} /></div>{/if}
            {#if loadedTraining && trainingLine}
              <p class="training-note side-notice" role="status">
                <Icon name={trainingVideosBusy(live.trainingVideos) ? "info" : "alert"} size={14} />{trainingLine}
              </p>
            {/if}

            <!-- Straight after On air, so Stop the sound now is never below
                 the fold; it stays shown even with the Sound box folded. -->
            <SoundPanel {items} {service} />

            <SideCard title="Screen" key="lampstand.present.screen" summary={showGuide ? "Safe area on" : undefined}>
              <Segmented options={modes} value={live.overlay} onchange={setOverlay} label="Screen" />
              <p class="side-line muted"><Icon name="monitor" size={14} />{output?.name ?? "No screen yet"}</p>
              <Switch checked={showGuide} label="Safe area guide" onchange={() => { showGuide = !showGuide; used("safe-area"); }} />
              <Switch checked={bugOn} disabled={!bugReady} label={bugReady ? "Logo bug" : "Logo bug (add a logo in Setup)"} onchange={toggleBug} />
              {#if onidle}<button type="button" class="side-row" onclick={onidle}><Icon name="image" size={14} />Idle screen<span class="side-hint">Change</span></button>{/if}
            </SideCard>

            <SideCard title="The room">
              {#if pill}
                <!-- The OBS controller: what OBS has on air and what the next
                     item switches to. Opens the drawer of scenes, preview and
                     mutes. The connection's status sits quietly in the footer. -->
                <button
                  bind:this={obsToggle}
                  class="obs-control"
                  class:open={obsOpen}
                  onclick={() => setObsOpen(!obsOpen)}
                  aria-expanded={obsOpen}
                  aria-controls="obs-drawer"
                  title="Control OBS: switch scenes, preview them, mute audio (O)"
                >
                  <span class="obs-tag"><Icon name="remote" size={14} />OBS<Dot tone={pill.tone} size={6} /></span>
                  {#if obsOn && live.obs?.program_scene}
                    <span class="on-air"><span class="sr">On air: </span>{live.obs.program_scene}</span>
                    {#if nextObsScene}<span class="next">Next: {nextObsScene}</span>{/if}
                  {:else}
                    <span class="on-air">{obsOn ? "Scenes" : "Not connected"}</span>
                  {/if}
                  <span class="chevron" aria-hidden="true"><Icon name="next" size={12} /></span>
                </button>
              {/if}
              <button type="button" class="side-row" title="Quick add a song or scripture after what is on air" onclick={() => { quickAddOpen = true; used("quick-add"); }}><Icon name="plus" size={14} />Quick add<kbd>/</kbd></button>
            </SideCard>

            {#if live.loaded && liveSection}
              <LiveLoop loaded={live.loaded} section={liveSection} />
            {/if}

            <SideCard title="Remotes" key="lampstand.present.remotes" open={false} summary={clicker ? "Clicker on" : undefined}>
              <Switch checked={clicker} label="Clicker mode" onchange={toggleClicker} />
              <button type="button" class="side-row" onclick={() => (connectOpen = true)}><Icon name="phone" size={14} />Connect a phone or tablet</button>
            </SideCard>

            <SideCard title="Clear" key="lampstand.present.clear" open={false}>
              <div class="clear">
                <Button onclick={() => command("ClearText")} title="Clear the text and keep the background">Text</Button>
                <Button onclick={() => command("ClearAll")} title="Clear the text, background and overlay">Everything</Button>
              </div>
            </SideCard>

            <SideCard title="Keys" key="lampstand.present.keys" open={false}>
              <dl class="keys">
                {#each keys as [k, what]}
                  <dt>{k}</dt>
                  <dd>{what}</dd>
                {/each}
                <dt>F</dt>
                <dd>Focus: the cards only</dd>
                {#if pill}
                  <dt>O</dt>
                  <dd>Control OBS</dd>
                {/if}
                <dt>{mod}1 · {mod}2 · {mod}\</dt>
                <dd>Plan · Present · switch between them</dd>
              </dl>
            </SideCard>

            {#if tip}
              <TipCard {tip} ondismiss={() => dismissTip(tip.id)} onall={() => ontips?.()} onaction={() => tipAction(tip)} onsnooze={() => snoozeTip(tip)} />
            {/if}

            {#if confirmClose}
              <div class="close">
                <span class="close-ask">Something is on air. Close the service?</span>
                <span class="close-buttons">
                  <Button size="small" variant="danger" onclick={closeService}>Close service</Button>
                  <Button size="small" variant="ghost" onclick={() => (confirmClose = false)}>Keep open</Button>
                </span>
              </div>
            {:else}
              <button type="button" class="side-row quiet" title="Put the service away; it stays in Plan" onclick={askClose}><Icon name="x" size={14} />Close service</button>
            {/if}
          </div>
        </aside>
        {/if}
      </section>
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
    display: flex;
    align-items: center;
    gap: 0.5rem;
    width: 100%;
    min-height: 2.5rem;
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
  .close {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    padding: 0.25rem 0.25rem 0;
  }
  .close-buttons {
    display: flex;
    gap: 0.4rem;
  }
  .close-ask {
    font-size: 0.8125rem;
    color: var(--muted);
  }
  .grid {
    position: relative;
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr);
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
  .pick-note {
    margin-top: 0.4rem;
    font-size: 0.8rem;
  }
  .pick-note:empty {
    display: none;
  }
  .training-note {
    display: flex;
    align-items: flex-start;
    gap: 0.35rem;
    min-width: 0;
    font-size: 0.8rem;
    color: var(--muted);
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
  .state {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    font-weight: 700;
    letter-spacing: 0.04em;
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
    font-size: 0.8125rem;
    font-weight: 600;
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
    border: 1px solid transparent;
    background: none;
    color: var(--muted);
    cursor: pointer;
  }
  .loop-toggle:hover {
    color: var(--text);
    border-color: var(--line-strong);
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
  .pick-head .quiet-link {
    margin-top: 0.35rem;
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
  /* A song's sections, coloured along the top of the caption, so verses,
     choruses, pre-choruses and bridges stand out down the wall. */
  .slide.verse {
    --tone: var(--section-verse);
  }
  .slide.chorus {
    --tone: var(--section-chorus);
  }
  .slide.prechorus {
    --tone: var(--section-prechorus);
  }
  .slide.bridge {
    --tone: var(--section-bridge);
  }
  .slide.verse .caption,
  .slide.chorus .caption,
  .slide.prechorus .caption,
  .slide.bridge .caption {
    box-shadow: inset 0 3px 0 var(--tone);
    background: color-mix(in srgb, var(--tone) 10%, transparent);
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

  .slide.next {
    box-shadow: 0 0 0 2px var(--gold);
  }
  .wall-where {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* A small picture of the main screen, always on, with its name and safe area. */
  .monitor {
    margin: 0;
  }
  .monitor-screen {
    border-radius: var(--radius);
    overflow: hidden;
    box-shadow: 0 0 0 2px var(--line);
  }
  .monitor-screen.on {
    box-shadow: 0 0 0 2px var(--live);
  }
  .wall-now {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    min-width: 0;
  }
  /* The wall on the left, the sidecard on the right, each scrolling on its own. */
  .wall-pane {
    position: relative;
    flex-direction: row;
  }
  .sidecard {
    flex: 0 0 21rem;
    min-height: 0;
    overflow-y: auto;
    padding: 0.75rem 1rem 1.5rem 0;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }
  .sidecard .no-screen {
    margin-bottom: 0;
  }
  /* Wide: the two parts are one column, with the notices above On air. */
  .side-onair,
  .side-rest {
    display: contents;
  }
  .side-notice {
    order: -1;
  }
  .sidecard :global(.sound) {
    margin: 0;
    background: var(--panel);
    border-color: transparent;
  }
  .transport {
    display: grid;
    grid-template-columns: 1fr 1.6fr 1fr;
    gap: 0.4rem;
  }
  .transport :global(button) {
    height: 2.75rem;
    justify-content: center;
    padding: 0 0.5rem;
  }
  .side-line {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.8125rem;
  }
  .side-row {
    font: inherit;
    font-size: 0.85rem;
    color: var(--text);
    display: flex;
    align-items: center;
    gap: 0.5rem;
    width: 100%;
    min-height: 2.25rem;
    padding: 0;
    border: none;
    background: none;
    text-align: left;
    cursor: pointer;
  }
  .side-row :global(.icon) {
    color: var(--muted);
  }
  .side-row:hover {
    color: var(--gold-hover);
  }
  .side-row.quiet {
    color: var(--muted);
    padding: 0 0.25rem;
  }
  .side-row.quiet:hover {
    color: var(--text);
  }
  .side-hint,
  .side-row kbd {
    margin-left: auto;
    color: var(--muted);
  }
  .side-row kbd {
    font: inherit;
    font-size: 0.72em;
    padding: 0 0.3rem;
    border: 1px solid currentColor;
    border-radius: 3px;
  }
  .clear {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 0.4rem;
  }
  .clear :global(button) {
    justify-content: center;
  }
  .keys {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 0.35rem 0.75rem;
    font-size: 0.8rem;
  }
  .keys dt {
    font-family: ui-monospace, Menlo, monospace;
    white-space: nowrap;
  }
  .keys dd {
    margin: 0;
    color: var(--muted);
  }
  /* A narrow window: the sidecard above the wall, On air beside a column of
     the rest. The wall scrolls on its own, as it does when wide, and the
     sidecard is a bounded block: On air stays put and the rest scrolls,
     Sound first. So following the service down the wall never takes
     Previous, Next, Blank or Stop the sound now off screen (nor the Focus
     pill, which sits over the wall). */
  @media (max-width: 900px) {
    .wall-pane {
      flex-direction: column;
    }
    .sidecard {
      order: -1;
      flex: 0 1 auto;
      max-height: max(45%, 18.5rem);
      padding: 0.75rem 1rem;
      border-bottom: 1px solid var(--line);
      flex-direction: row;
      overflow: hidden;
    }
    .side-onair,
    .side-rest {
      flex: 1 1 0;
      min-width: 0;
      min-height: 0;
      overflow-y: auto;
    }
    .side-onair {
      display: block;
    }
    .side-rest {
      display: flex;
      flex-direction: column;
      gap: 0.6rem;
    }
    .side-notice {
      order: 0;
    }
    .side-rest > :global(.sound) {
      order: -1;
    }
    .side-rest > :global(*) {
      flex-shrink: 0;
    }
    .sidecard .monitor,
    .sidecard .wall-steps figure {
      max-width: 12rem;
    }
  }
  /* Focus: the cards only, with what is on air and the way out in a pill. */
  .focus-pill {
    position: absolute;
    top: 0.75rem;
    right: 1.25rem;
    z-index: 5;
    display: flex;
    align-items: center;
    gap: 0.6rem;
    max-width: calc(100% - 2.5rem);
    padding: 0.3rem 0.3rem 0.3rem 0.75rem;
    border: 1px solid var(--line-strong);
    border-radius: 999px;
    background: var(--panel);
    font-size: 0.75rem;
  }
  /* The smallest windows: the buttons keep their words, the key hints go. */
  @media (max-width: 780px) {
    .focus-pill :global(kbd) {
      display: none;
    }
  }
  /* Room above the first row, so the pill never covers a card. */
  .focus-pill ~ .wall-body {
    padding-top: 3.5rem;
  }
  .focus-where {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .wall-steps {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  .wall-steps-head {
    font-size: 0.8rem;
  }
  .wall-steps figure {
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    font-size: 0.75rem;
  }
  .wall-notes {
    margin-top: 0.4rem;
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
    font-size: 0.8rem;
    line-height: 1.45;
    white-space: pre-wrap;
  }
  .wall-body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0.75rem 1.25rem 2rem;
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }
  .wall-group {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }
  .wall-group.looping {
    border: 1px solid var(--text);
    border-radius: var(--radius-lg);
    padding: 0.25rem;
  }
  .wall-eyebrow {
    flex-direction: row;
    align-items: center;
    justify-content: space-between;
  }
  .wall-row {
    display: grid;
    grid-template-columns: 13rem minmax(0, 1fr);
    gap: 1rem;
    padding: 0.75rem;
    border: 1px solid transparent;
    border-radius: var(--radius-lg);
  }
  .wall-row.on-air {
    background: var(--panel);
  }
  .wall-row.viewed {
    border-color: var(--line);
  }
  .wall-row.played {
    opacity: 0.55;
  }
  .wall-row.played:hover,
  .wall-row.played:focus-within {
    opacity: 1;
  }
  .wall-label {
    display: flex;
    align-items: flex-start;
    gap: 0.5rem;
    min-width: 0;
  }
  .wall-title {
    min-width: 0;
  }
  .wall-title h3 {
    font-size: 0.95rem;
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .wall-title p {
    margin-top: 0.15rem;
    font-size: 0.8rem;
  }
  .on-air-tag {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    font-weight: 600;
    color: var(--live);
  }
  .wall-slides {
    margin-top: 0;
  }
  /* A narrow window: the label sits above the slides. */
  @media (max-width: 900px) {
    .wall-row {
      grid-template-columns: minmax(0, 1fr);
    }
  }
  .next {
    margin-top: 0.5rem;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    overflow: hidden;
  }
</style>
