<!-- Operator panel: the wall. Every item's slides in the order of
     service, the slide on air marked LIVE and the one Next shows marked
     NEXT. The sidebar beside it, Library & controls (the same name as
     Plan's), starts with the Library: a search box that adds a song or
     reading next, or keeps a deck or media ready (Quick add). Under Controls
     come what is on air with Previous, Next and Blank, Ready (Blank, Idle
     and what the service keeps one tap away, lib/ready.ts), the screen, the
     room (OBS, Sound, the group loop),
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
  import SlidePreview from "../../lib/kit/SlidePreview.svelte";
  import StatusPill from "../../lib/kit/StatusPill.svelte";
  import { kindIcon, kindLabel, kindName } from "../../lib/kit/items";
  import TipCard from "../../lib/kit/TipCard.svelte";
  import { nextTip, loadDismissed, saveDismissed, loadUsed, saveUsed, loadSnoozed, saveSnoozed, snooze, type Tip } from "../../lib/tips";
  import { loadNarrow, loadPreviewSize, loadSafeAreaGuide, saveNarrow, type PreviewSize } from "../../lib/presentPrefs";
  import LoopProgress from "../../lib/kit/LoopProgress.svelte";
  import SoundPanel from "./SoundPanel.svelte";
  import { musicLine, words, type SoundEntry } from "../../lib/audio";
  import ObsDrawer from "./ObsDrawer.svelte";
  import QuickAdd from "./QuickAdd.svelte";
  import { loadedItem, readyList, returnLine } from "../../lib/ready";
  import { loadMediaThumb } from "../../lib/mediaThumbLoader";
  import ObsNotice from "./ObsNotice.svelte";
  import { isConnected, isScreen, notice as obsNotice, obsPill, sceneNotice, upcomingScene } from "../../lib/obs";
  import Banner from "../../lib/kit/Banner.svelte";
  import { formatWhen } from "../../lib/when";
  import { newScreen, screenOffers, type ScreenOffer } from "../../lib/screenOffers";
  import { steppedDown, windowToDemote } from "../../lib/primary";
  import { isTyping } from "../../lib/shortcuts";
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

  /** Opens Settings › Screens (the no-screen prompt), Settings › OBS and Plan (the blank view); `onidle` opens where the idle screen is chosen, `ondevices` where devices are paired. */
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

  // Space or Right: Next. Left: Previous. Full stop: Black. 1 to 9 jump to
  // that verse; C, B, P, T, I, E to the next chorus, bridge, pre-chorus,
  // tag, intro or ending of the live song. Typing in a field never
  // triggers a shortcut. A switch (Logo bug, Safe area guide) is not a
  // field: Space stays Next there, and does not flip the switch.
  function onkeydown(e: KeyboardEvent) {
    const target = e.target as HTMLElement | null;
    if (isTyping(target)) return;
    // A dialog over the service (Connect a phone or tablet, Quick add) has
    // its own keys; none of them may move the screen.
    if (connectOpen || quickAddOpen || target?.closest?.('[aria-modal="true"]')) return;
    const off = shortcutsOff(streamDesk, !!live.loaded, !!target?.closest?.("#obs-drawer.docked"));
    if (e.ctrlKey && e.altKey && !e.metaKey && !off) {
      chordKey(e);
      return;
    }
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    if (off) return;
    if (live.loaded && e.key === "/") {
      e.preventDefault();
      quickAddReady = false;
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
      // The three screen keys sit together: . Blank, , Idle, ; Clear text.
      ".": "ToggleBlack",
      ",": "ToggleIdle",
      ";": "ClearText",
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
  /** Quick add opened from the Ready card: the Library's decks and media first, for Ready. */
  let quickAddReady = $state(false);
  function openQuickAdd(forReady: boolean) {
    quickAddReady = forReady;
    quickAddOpen = true;
    used("quick-add");
  }
  /** A deck or media from Quick add went to Ready; Shift+Enter shows it now, as a side-trip. */
  function readied(id: string | null, showNow: boolean, title: string) {
    quickAddOpen = false;
    live.status = `${title} is in Ready`;
    if (showNow && id) showReady(id);
  }
  /** The "Connect a phone or tablet" dialog (T101). */
  let connectOpen = $state(false);
  function quickAdded(id: string | null, showNow: boolean, title: string) {
    quickAddOpen = false;
    live.status = `Added ${title}`;
    if (showNow && id) show({ item: id, slide: 0, step: 0 });
  }
  let viewed = $derived(items.find((i) => i.id === viewing) ?? items.find((i) => i.slides.length > 0) ?? items[0]);
  /**
   * Narrower: the whole right column slims down, so the service on the wall
   * gets the room. One labelled button in On air's heading, kept on this
   * computer, so a church sets it once.
   */
  let narrow = $state(loadNarrow());
  function toggleNarrow() {
    narrow = !narrow;
    saveNarrow(narrow);
    live.status = narrow ? "The right side is narrower on this computer: more room for the service" : "The right side is wider on this computer";
  }

  /**
   * The modifier chord, for power users: hold Ctrl + Alt (Ctrl + Option on a
   * Mac, the same two keys on every computer) and, after a moment, every
   * control it reaches shows its key. The right hand stays on its home keys:
   * U I O and J K L are the tiles, in the same grid as on screen (I Idle,
   * U Blank, O Clear text, then the first three in Ready); , and . start the
   * item before or after the one on air; M stops the sound, P plays the
   * sound that waits for Play, / adds, ; opens the stream. Matched by key
   * position (e.code), so the grid keeps its shape on any keyboard layout.
   * Every key does what a labelled button does; a volunteer who never holds
   * the chord never sees any of it. VoiceOver on a Mac also uses Ctrl +
   * Option, so while it runs the chord is VoiceOver's.
   */
  let chord = $state(false);
  let chordTimer: ReturnType<typeof setTimeout> | undefined;
  const SCREEN_CODES: Record<string, Command> = { KeyU: "ToggleBlack", KeyI: "ToggleIdle", KeyO: "ClearText" };
  const READY_CODES = ["KeyJ", "KeyK", "KeyL"];
  const READY_LETTERS = ["J", "K", "L"];
  /** The sound that waits for Play, from Sound in the press block. */
  let waiting: SoundEntry | null = $state(null);
  function chordOff() {
    clearTimeout(chordTimer);
    chord = false;
  }
  function chordKey(e: KeyboardEvent) {
    if (e.key === "Control" || e.key === "Alt") {
      clearTimeout(chordTimer);
      chordTimer = setTimeout(() => (chord = true), 350);
      return;
    }
    const screen = SCREEN_CODES[e.code];
    const r = READY_CODES.indexOf(e.code);
    let done = true;
    if (screen) command(screen);
    else if (r >= 0 && readyTiles[r]) showReady(readyTiles[r].item.id);
    else if ((e.code === "Comma" && jumpBack) || (e.code === "Period" && jumpOn)) {
      const id = (e.code === "Comma" ? jumpBack : jumpOn) as string;
      show({ item: id, slide: 0, step: 0 });
    } else if (e.code === "Slash") openQuickAdd(false);
    else if (e.code === "Semicolon" && pill) setObsOpen(!obsOpen);
    // The panic stop always works, whatever this device thinks is playing.
    else if (e.code === "KeyM") sound("Panic").catch(report);
    else if (e.code === "KeyP" && waiting) {
      const w = waiting;
      sound({ Play: { item: w.item, backing: w.backing } }).catch(report);
    } else done = false;
    if (done) e.preventDefault();
    chord = true;
  }
  function onkeyup(e: KeyboardEvent) {
    if (e.key === "Control" || e.key === "Alt") chordOff();
  }
  let liveIndex = $derived(items.findIndex((i) => i.id === live.live?.item));
  /** The items the chord's , and . start: before and after the one on air. */
  let jumpFrom = $derived(live.live?.item ?? viewed?.id ?? null);
  let jumpBack = $derived(neighbourItem(items, jumpFrom, -1));
  let jumpOn = $derived(neighbourItem(items, jumpFrom, 1));
  /** The section on air, if the live item sits under a header. */
  let liveSection = $derived(live.loaded?.sections.find((s) => s.items.includes(live.live?.item ?? "")) ?? null);
  /** While the group on air loops and the slide is showing: how long each slide stays up, else null. */
  let loopSeconds = $derived(live.live && live.overlay === "None" ? (liveSection?.seconds ?? null) : null);
  /** Changes whenever a new slide or build goes up, so the bar starts again. */
  let loopKey = $derived(live.live ? `${live.live.item}:${live.live.slide}:${live.live.step}` : "");
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
  // A ready item kept out of the order is on air too, on a side-trip.
  let liveItem = $derived(liveIndex >= 0 ? items[liveIndex] : live.live ? (loadedItem(live.loaded, live.live.item) ?? null) : null);
  let tip = $derived(
    nextTip(
      { loaded: live.loaded ?? null, liveItem: live.live?.item ?? null, liveSlides: liveItem?.slides.length ?? 0, hasLogo: !!live.settings?.brand?.logo, paired: live.team?.paired.length, realUse: live.realUse },
      new Set([...dismissedTips, ...usedTips]),
      undefined,
      snoozedTips,
    ),
  );
  let upcoming = $derived(live.loaded ? nextSlide(live.loaded, live.live, live.returnsTo) : null);
  // The row on air scrolls into view when the service moves to
  // another item, so Next never leaves the operator looking elsewhere.
  $effect(() => {
    const item = live.live?.item;
    if (!item) return;
    tick().then(() => document.querySelector(".wall-row.on-air")?.scrollIntoView({ block: "nearest", behavior: "smooth" }));
  });
  // A deck on air gets the presenter view, whose Next is the next click.
  let presenting = $derived(!!(liveItem && live.live && live.liveSlide && isDeckSlide(live.liveSlide)));
  let clickNext = $derived(live.loaded ? nextClick(live.loaded, live.live, live.returnsTo) : null);
  /** A deck slide with builds is on air: the one time the wall shows the screen and the next click, because the cards show each slide fully built. */
  let stepped = $derived(!!live.live && (live.liveSlide?.steps ?? 0) > 0);
  // The picture of what is on air, top right: small (the default), large
  // (the sidecard widens on a wide window, so the picture is big) or off.
  // Remembered per computer, as the cards' folds are.
  // Chosen in Settings › Screens (lib/presentPrefs.ts), not here.
  let previewSize: PreviewSize = $state(loadPreviewSize());
  // The picture of what is on air is large for a deck (the presenter view)
  // and for an image or video, which fills the big preview.
  let liveWidth = $derived(presenting || previewSize === "large" || mediaOf(live.shown?.item) ? 1280 : 480);

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
    const own = item ? service?.ready?.[item]?.source : undefined;
    const kind = item ? (service?.items[item]?.kind ?? (own && own !== "Order" && "Own" in own ? own.Own.kind : undefined)) : undefined;
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
    // The Ready card shows a picture of the first few ready items.
    for (const r of readyTiles) {
      const first = r.item.slides[0];
      if (first && hasPicture(r.item.id, first)) want.push([r.item.id, 0, first.steps, THUMB_WIDTH]);
    }
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
  let showGuide = $state(loadSafeAreaGuide());




  function kindOf(id: string) {
    const item = service?.items[id];
    if (item) return kindName(item.kind);
    const own = service?.ready?.[id]?.source;
    if (!own || own === "Order") return null;
    return "Own" in own ? kindName(own.Own.kind) : "Deck";
  }

  /** The deck an item of the order shows, if it is a deck. */
  function deckOf(id: string): string | null {
    const kind = service?.items[id]?.kind;
    return kind && "Deck" in kind ? kind.Deck.deck : null;
  }
  /** Slides kept ready one by one, by deck, with each entry's ID. */
  let slideEntries = $derived(
    Object.entries(service?.ready ?? {}).flatMap(([id, e]) => (e.source !== "Order" && "Slide" in e.source ? [{ id, ...e.source.Slide }] : [])),
  );
  let keptSlides = $derived(
    slideEntries.reduce<Record<string, number[]>>((by, e) => ((by[e.deck] ??= []).push(e.slide), by), {}),
  );
  /** A slide kept ready on its own is on air: its builds play by themselves. */
  let autoBuilding = $derived(!!live.live && slideEntries.some((e) => e.id === live.live?.item));
  async function toggleSlide(deck: string, slide: number, title: string) {
    const loaded = live.loaded;
    if (!loaded) return;
    const entry = slideEntries.find((e) => e.deck === deck && e.slide === slide);
    try {
      if (entry) await request({ area: "Service", request: { RemoveReady: { service: loaded.service, item: entry.id } } }, "Service");
      else await request({ area: "Service", request: { AddReadySlide: { service: loaded.service, deck, slide } } }, "Service");
      live.status = entry ? `${title} · slide ${slide + 1} is out of Ready` : `${title} · slide ${slide + 1} is in Ready`;
    } catch (e) {
      report(e);
    }
  }

  // Ready: Blank and Idle first, then what the service keeps one tap away.
  // Tapping one is a side-trip; Next past its end goes back to the service.
  let ready = $derived(readyList(live.loaded));
  /** The first few show as pictures, the rest as rows. */
  const READY_TILES = 3;
  let readyTiles = $derived(ready.slice(0, READY_TILES));
  let readyRows = $derived(ready.slice(READY_TILES));
  let pinned = $derived(new Set((live.loaded?.ready ?? []).filter((r) => r.in_order).map((r) => r.item.id)));
  let backLine = $derived(live.onReady ? returnLine(live.loaded, live.returnsTo) : null);
  function showReady(id: string) {
    used("ready");
    command({ ShowReady: id });
  }
  async function setPinned(item: string, on: boolean, title: string) {
    const loaded = live.loaded;
    if (!loaded) return;
    try {
      await request({ area: "Service", request: { PinReady: { service: loaded.service, item, ready: on } } }, "Service");
      live.status = on ? `${title} is in Ready` : `${title} is out of Ready`;
    } catch (e) {
      report(e);
    }
  }
  async function takeOffReady(id: string, inOrder: boolean, title: string) {
    if (inOrder) return setPinned(id, false, title);
    const loaded = live.loaded;
    if (!loaded) return;
    try {
      await request({ area: "Service", request: { RemoveReady: { service: loaded.service, item: id } } }, "Service");
      live.status = `${title} is out of Ready`;
    } catch (e) {
      report(e);
    }
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

<svelte:window {onkeydown} {onkeyup} onblur={chordOff} />

<!-- A key of the modifier chord, shown on its control while the chord is held. -->
{#snippet chordBadge(k: string)}
  {#if chord}<span class="chord-key" aria-hidden="true">{k}</span>{/if}
{/snippet}

<!-- Look: what the room sees. The picture, what is live, what Next shows,
     and whether all is set. Nothing to press here but a fix. -->
{#snippet onAirBody()}
    {#if previewSize !== "off"}
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
          idleVideo={look.idleInfo.video}
          brand={look.brand}
          empty="Nothing on air"
        />
        {#if loopSeconds}<LoopProgress seconds={loopSeconds} slideKey={loopKey} />{/if}
      </div>
    </figure>
    {/if}
    <div class="wall-now">
      <span class="state"><Dot tone={live.overlay === "None" && live.live ? "live" : "neutral"} size={8} />{stateLabel[live.overlay]}</span>
      <span class="muted wall-where">
        {#if live.overlay === "Idle" && liveItem && live.live}
          {"Idle screen · Show returns to "}{liveItem.title} · {live.liveSlide?.label ?? live.live.slide + 1}{#if upcoming}{" · Next shows "}{upcoming.title} · {upcoming.slide.label ?? upcoming.cursor.slide + 1}{/if}
        {:else if liveItem && live.live}{liveItem.title} · {liveItem.slides.length === 0 ? "Music only" : (live.liveSlide?.label ?? live.live.slide + 1)}{:else}Nothing on air. Next starts the service.{/if}
      </span>
      {#if backLine && live.overlay !== "Idle"}<span class="muted wall-back"><Icon name="prev" size={12} />{backLine}</span>{/if}
      {#if upcoming && live.live && live.overlay === "None"}<span class="faint look-next">Next: <strong>{upcoming.title === liveItem?.title ? (upcoming.slide.label ?? upcoming.cursor.slide + 1) : `${upcoming.title} · ${upcoming.slide.label ?? upcoming.cursor.slide + 1}`}</strong></span>{/if}
    </div>
    {#if stepped && live.overlay !== "Idle" && liveItem && live.live && live.liveSlide}
      {@const left = live.liveSlide.steps - live.live.step}
      <section class="wall-steps" aria-label="Builds on this slide">
        <p class="wall-steps-head muted">{#if autoBuilding}{left === 0 ? "Fully built. Next goes back to the service." : `${left} ${left === 1 ? "build" : "builds"} left, playing by themselves`}{:else}{left === 0 ? "Fully built. Next goes to the next slide." : `${left} ${left === 1 ? "click" : "clicks"} left on this slide`}{/if}</p>
      </section>
    {/if}
{/snippet}

<!-- Press: everything you press during a service, in one block. Previous
     and Next, then the tiles: Idle, Blank and Clear text as pictures of what
     the screens will become, in the order of their keys , . ; then what was
     added to Ready. Sound joins while it plays; Stream for a church with OBS. -->
{#snippet pressBody()}
    <div class="transport">
      <!-- The arrow keys are what people reach for, and what a clicker sends;
           Space still works for Next. -->
      <Button icon="prev" onclick={() => command("Previous")} title="Previous (← or Page Up)" shortcut="←">Previous</Button>
      <Button variant="primary" onclick={() => command("Next")} title="Next (→, Space or Page Down)" shortcut="→">Next</Button>
    </div>
    <div class="ready-screens" aria-label="Screens">
      <button type="button" class="screen-pick" class:on={live.overlay === "Idle"} aria-pressed={live.overlay === "Idle"} onclick={() => command("ToggleIdle")} title="Put the idle screen up ({look.idleInfo.video ? "a looping video" : look.idleInfo.label}) without ending the service, until you tap again or show a slide (,)">
        {@render chordBadge("I")}
        <SlidePreview overlay="Idle" idle={look.idle} idleVideo={look.idleInfo.video} idleTag={false} size="small" />
        <span class="screen-caption">
          <strong>Idle</strong><kbd>,</kbd>
        </span>
      </button>
      <button type="button" class="screen-pick blank" class:on={live.overlay === "Black"} aria-pressed={live.overlay === "Black"} onclick={() => command("ToggleBlack")} title="Black screens, until you tap again (.)">
        {@render chordBadge("U")}
        <SlidePreview overlay="Black" size="small" />
        <span class="screen-caption"><strong>Blank</strong><kbd>.</kbd></span>
      </button>
      <button type="button" class="screen-pick clear-text" onclick={() => command("ClearText")} title="Take the words off the screen and keep the background, until the next slide (;)">
        {@render chordBadge("O")}
        <span class="clear-pic" aria-hidden="true"></span>
        <span class="screen-caption"><strong>Clear text</strong><kbd>;</kbd></span>
      </button>
    </div>
    {#if ready.length > 0}
      <ul class="plain ready-tiles" aria-label="In Ready">
        {#each readyTiles as r, t (r.item.id)}
          {@const onAir = live.live?.item === r.item.id && live.overlay === "None"}
          {@const first = r.item.slides[0]}
          <li class="ready-tile" class:on-air={onAir}>
            <button type="button" class="ready-pick" onclick={() => showReady(r.item.id)} title="Show {r.item.title} now; Next comes back to the service">
              {@render chordBadge(READY_LETTERS[t])}
              <SlidePreview lines={first?.lines ?? []} footer={first?.footer ?? null} image={first ? thumbFor(r.item.id, 0, first.steps) : null} size="small" empty={r.item.title} />
              <span class="ready-caption">
                {#if onAir}<StatusPill tone="live">LIVE</StatusPill>{/if}
                <span class="ready-title">{r.item.title}</span>
              </span>
            </button>
            <button type="button" class="ready-remove on-tile" onclick={() => takeOffReady(r.item.id, r.in_order, r.item.title)} title={r.in_order ? `Take ${r.item.title} out of Ready (it stays in the order)` : `Take ${r.item.title} out of Ready`} aria-label="Take {r.item.title} out of Ready">
              <Icon name="x" size={12} />
            </button>
          </li>
        {/each}
      </ul>
      {#if readyRows.length > 0}
        <ul class="plain ready-list" aria-label="Also in Ready">
          {#each readyRows as r (r.item.id)}
            {@const kind = kindOf(r.item.id)}
            {@const onAir = live.live?.item === r.item.id && live.overlay === "None"}
            <li class="ready-row" class:on-air={onAir}>
              <button type="button" class="ready-show" onclick={() => showReady(r.item.id)} title="Show {r.item.title} now; Next comes back to the service">
                <Icon name={kind ? kindIcon[kind] : "file"} size={14} />
                <span class="ready-title">{r.item.title}</span>
                {#if onAir}<StatusPill tone="live">LIVE</StatusPill>{:else if !r.in_order}<span class="side-hint">Only here</span>{/if}
              </button>
              <button type="button" class="ready-remove" onclick={() => takeOffReady(r.item.id, r.in_order, r.item.title)} title={r.in_order ? `Take ${r.item.title} out of Ready (it stays in the order)` : `Take ${r.item.title} out of Ready`} aria-label="Take {r.item.title} out of Ready">
                <Icon name="x" size={12} />
              </button>
            </li>
          {/each}
        </ul>
      {/if}
    {/if}
    <button type="button" class="add-ready" title="Add to Ready: a deck, picture or video from the Library, one tap away during the service. Add to Ready on an item in the service puts it here too." onclick={() => openQuickAdd(true)}><Icon name="plus" size={12} />Add to Ready</button>
    <SoundPanel {items} {service} {chord} bind:waiting />
    {#if pill}
      <!-- Only for a church that streams with OBS: what OBS has on air, and
           the drawer of scenes, preview and mutes (O). -->
      <button
        bind:this={obsToggle}
        class="obs-control"
        class:open={obsOpen}
        onclick={() => setObsOpen(!obsOpen)}
        aria-expanded={obsOpen}
        aria-controls="obs-drawer"
        title="Stream: switch OBS scenes, preview them, mute audio (O)"
      >
        {@render chordBadge(";")}
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
{/snippet}

<!-- Check: one line that answers "am I all set?". The first thing that
     needs the volunteer, with the button that fixes it; otherwise All set. -->
{#snippet checkRow()}
  {#if screensOn === 0}
    <div class="check warn" role="status">
      <Icon name="alert" size={15} />
      <div>
        <strong>No screen is on.</strong> Nothing reaches the projector.
        <div class="check-fix">
          {#each offers as offer (offer.label)}
            <Button size="small" variant="primary" icon="monitor" onclick={() => turnOn(offer)}>{offer.label}</Button>
          {/each}
          {#if onsetup}<Button size="small" icon="monitor" onclick={onsetup}>Set up screens</Button>{/if}
        </div>
      </div>
    </div>
  {:else if obsNotice(live.obs) || sceneNotice(live.obs)}
    <div class="check-obs"><ObsNotice view={live.obs} onsetup={onobs} /></div>
  {:else if pill?.tone === "warn"}
    <div class="check warn" role="status">
      <Icon name="alert" size={15} />
      <div>
        <strong>{pill.text}.</strong>
        {#if onobs}<div class="check-fix"><Button size="small" onclick={onobs}>Open Settings › OBS</Button></div>{/if}
      </div>
    </div>
  {:else if loadedTraining && trainingLine}
    <p class="check info" role="status"><Icon name={trainingVideosBusy(live.trainingVideos) ? "info" : "alert"} size={14} />{trainingLine}</p>
  {:else}
    <p class="check ok" role="status"><Icon name="checkCircle" size={14} /><strong>All set</strong><span class="muted">· {output?.name ?? "the screen"} is on</span></p>
  {/if}
{/snippet}

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
    {#if mark === "live" && loopSeconds}<LoopProgress seconds={loopSeconds} slideKey={loopKey} />{/if}
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
  <div class="operate" inert={paused} class:paused class:chording={chord}>
    {#if chord}
      <p class="chordbar" role="status">Modifier chord held: <strong>Ctrl + Alt</strong> (Ctrl + Option on a Mac). Press a key shown on a button.</p>
    {/if}
    <div class="grid">
      {#if connectOpen}
        <ConnectRemote onclose={() => (connectOpen = false)} />
      {/if}
      {#if quickAddOpen && live.loaded}
        <QuickAdd
          service={live.loaded.service}
          order={items.map((i) => i.id)}
          liveItem={live.live?.item ?? null}
          readyIds={(live.loaded.ready ?? []).map((r) => r.item.id)}
          forReady={quickAddReady}
          {keptSlides}
          onclose={() => (quickAddOpen = false)}
          onadded={quickAdded}
          onreadied={readied}
        />
      {/if}
      {#if obsOpen}
        <ObsDrawer view={live.obs} onclose={() => setObsOpen(false)} onsetup={onobs} />
      {/if}
      <!-- The wall: every item's slides at once, in the order of service,
           so any slide is a click away. The LIVE and NEXT marks on the
           cards say what is on air and what Next shows. The sidecard beside
           it holds the screen, the controls and the room, as cards that
           fold; F or its Hide button hides it, leaving a pill with the panic controls. -->
      <section class="pane wall-pane" aria-label="The whole service">
        <div class="wall-body">
          <!-- A quick tip, at the top of the wall and only while nothing is on
               air: never mid-service, and never in the right column's way. -->
          {#if tip && !live.live}
            <div class="wall-tip">
              <TipCard {tip} ondismiss={() => dismissTip(tip.id)} onall={() => ontips?.()} onaction={() => tipAction(tip)} onsnooze={() => snoozeTip(tip)} />
            </div>
          {/if}
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
                      {#if item.id === jumpBack}{@render chordBadge(",")}{:else if item.id === jumpOn}{@render chordBadge(".")}{/if}
                      <span class="num index" class:current={i === liveIndex}>{i + 1}</span>
                      <span class:current={i === liveIndex}>
                        <Icon name={kind ? kindIcon[kind] : "file"} label={kind ? kindLabel[kind] : undefined} />
                      </span>
                      <div class="wall-title">
                        <h3>{item.title}</h3>
                        <p class="muted wall-count">
                          {item.slides.length === 0 ? (kind === "Sound" ? musicLine(item) : "Nothing to show") : `${item.slides.length} ${item.slides.length === 1 ? "slide" : "slides"}`}
                          {#if kind === "Sound" && item.slides.length === 0}
                            <!-- Music only has no slide to click: its Play is right here. -->
                            {#if live.sound?.playing?.item === item.id}
                              <button type="button" class="wall-play" onclick={() => sound("Stop").catch(report)} title="Fade {item.title} out">Fade out</button>
                            {:else}
                              <button type="button" class="wall-play" onclick={() => sound({ Play: { item: item.id, backing: false } }).catch(report)} title="Play {item.title} now"><Icon name="play" size={12} />Play</button>
                            {/if}
                          {/if}
                          {#if item.slides.length > 0}
                            <button
                              type="button"
                              class="pin"
                              class:on={pinned.has(item.id)}
                              aria-pressed={pinned.has(item.id)}
                              title={pinned.has(item.id) ? `${item.title} is in Ready. Click to take it out` : `Optional: add ${item.title} to Ready, one tap away without losing your place`}
                              onclick={() => setPinned(item.id, !pinned.has(item.id), item.title)}
                            ><Icon name="star" size={12} />{pinned.has(item.id) ? "In Ready" : "Add to Ready"}</button>
                          {/if}
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
                          {@const deckId = deckOf(item.id)}
                          <li class="slide-cell">
                            {@render slideCard(item.id, slide, s, slideMark(item.id, s, live.live, upcoming?.cursor))}
                            {#if deckId}
                              {@const kept = (keptSlides[deckId] ?? []).includes(s)}
                              <button
                                type="button"
                                class="keep-slide"
                                class:on={kept}
                                aria-pressed={kept}
                                title={kept ? `Slide ${s + 1} is in Ready. Click to take it out` : `Add just this slide to Ready; its builds play by themselves`}
                                aria-label={kept ? `Take slide ${s + 1} of ${item.title} out of Ready` : `Add slide ${s + 1} of ${item.title} to Ready`}
                                onclick={() => toggleSlide(deckId, s, item.title)}
                              ><Icon name="star" size={11} />{kept ? "In Ready" : "Add slide to Ready"}</button>
                            {/if}
                          </li>
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
          <!-- Add, at the end of the service: a song or reading after what is
               on air, or a deck or media to Ready (/). -->
          <button type="button" class="wall-add" title="Add a song or reading after what is on air, or a deck or media to Ready (/)" onclick={() => openQuickAdd(false)}>
            {@render chordBadge("/")}
            <Icon name="plus" size={14} /><span>Add a song or reading</span><kbd>/</kbd>
          </button>
        </div>
        <!-- The right column, the same every time, for a volunteer who comes
             once a month. Look up: On air, what the room sees. Press down:
             everything you press. Nothing in it hides or moves. -->
        <aside id="library-and-controls" class="sidecard" class:narrow class:large-preview={previewSize === "large" && !narrow} aria-label="On air and controls">
          <section class="col-onair" aria-label="On air">
            <div class="col-head look-head">
              <h3>On air</h3>
              <button type="button" class="size-toggle" onclick={toggleNarrow} title={narrow ? "Make the right side wider again; kept on this computer" : "Make the right side narrower, for more room for the service; kept on this computer"}>{narrow ? "Wider" : "Narrower"}</button>
            </div>
            {@render onAirBody()}
            {@render checkRow()}
          </section>
          <section id="ready-tray" class="col-press" aria-label="Controls and Ready">
            {@render pressBody()}
          </section>
        </aside>
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
    position: relative;
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
    position: relative;
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
    position: relative;
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
  /* The right column: On air at the top, Library & controls scrolling in
     the middle, Ready at the bottom. Nothing in it hides or moves. */
  /* Calm: quiet sentence-case names, space between parts, no rules. */
  .col-head {
    margin: 0 0 0.35rem;
    font-size: 0.8rem;
    font-weight: 600;
    color: var(--faint);
  }
  .check {
    margin: 0;
    display: flex;
    align-items: flex-start;
    gap: 0.45rem;
    font-size: 0.875rem;
  }
  .check.ok {
    align-items: center;
    color: var(--muted);
  }
  .check.ok :global(svg),
  .check.ok strong {
    color: var(--ready-text, #5cc16f);
    font-weight: 600;
  }
  .check.info {
    color: var(--muted);
    font-size: 0.8125rem;
  }
  .check.warn {
    padding: 0.55rem 0.65rem;
    border-radius: var(--radius);
    background: var(--warn-soft, rgba(245, 165, 36, 0.12));
    box-shadow: inset 0 0 0 1px var(--warn, #f5a524);
  }
  .check.warn > :global(svg) {
    color: var(--warn, #f5a524);
    flex-shrink: 0;
    margin-top: 0.1rem;
  }
  .check-fix {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem;
    margin-top: 0.45rem;
  }
  .col-onair {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    padding: 0.75rem 1rem 0.5rem 0;
  }
  /* A short window: a smaller picture, so the press block stays in view. */
  @media (max-height: 880px) {
    .col-onair :global(.monitor) {
      width: 80%;
      align-self: center;
    }
  }
  /* Press: one block of everything you press, below what the room sees. */
  .col-press {
    flex: 0 1 auto;
    min-height: 0;
    max-height: 68%;
    overflow-y: auto;
    padding: 0.75rem 1rem 0.9rem 0;
    border-top: 1px solid var(--line);
    display: flex;
    flex-direction: column;
    gap: 0.55rem;
  }
  .look-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .look-head h3 {
    margin: 0;
    font: inherit;
  }
  .size-toggle {
    font: inherit;
    font-size: 0.74rem;
    font-weight: 500;
    padding: 0.1rem 0.5rem;
    color: var(--faint);
    background: none;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    cursor: pointer;
  }
  .size-toggle:hover {
    color: var(--text);
    border-color: var(--line-strong);
  }
  .look-next {
    font-size: 0.8rem;
  }
  .look-next strong {
    color: var(--muted);
    font-weight: 500;
  }
  .add-ready {
    font: inherit;
    font-size: 0.78rem;
    align-self: flex-start;
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0;
    color: var(--faint);
    background: none;
    border: none;
    cursor: pointer;
  }
  .add-ready:hover {
    color: var(--text);
  }
  /* Add, at the end of the service on the wall. */
  .wall-add {
    position: relative;
    align-self: flex-start;
    font: inherit;
    font-size: 0.88rem;
    display: inline-flex;
    align-items: center;
    gap: 0.45rem;
    margin-top: 0.5rem;
    padding: 0.5rem 0.8rem;
    color: var(--muted);
    background: none;
    border: 1px dashed var(--line-strong);
    border-radius: var(--radius);
    cursor: pointer;
  }
  .wall-add:hover {
    color: var(--text);
    border-color: var(--gold-hover);
  }
  .wall-add kbd {
    font: inherit;
    font-size: 0.72em;
    padding: 0 0.3rem;
    border: 1px solid currentColor;
    border-radius: 3px;
    opacity: 0.8;
  }
  /* The modifier chord: a gold strip says it is held, and each control it
     reaches shows its key in a gold badge at its top left corner. */
  .chordbar {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    z-index: 30;
    margin: 0;
    padding: 0.3rem 0.9rem;
    font-size: 0.8rem;
    color: var(--canvas);
    background: var(--gold);
  }
  .chord-key {
    position: absolute;
    top: -0.45rem;
    left: -0.35rem;
    z-index: 4;
    min-width: 1.35rem;
    padding: 0.05rem 0.3rem;
    border-radius: 5px;
    background: var(--gold);
    color: var(--canvas);
    font: 700 0.72rem/1.2 ui-monospace, Menlo, monospace;
    text-align: center;
    box-shadow: 0 2px 6px rgba(0, 0, 0, 0.5);
    pointer-events: none;
  }
  .chording .screen-pick,
  .chording .ready-pick,
  .chording .obs-control,
  .chording .wall-label {
    position: relative;
  }
  .sidecard {
    flex: 0 0 21rem;
    transition: flex-basis 0.25s ease;
    min-height: 0;
    padding: 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }
  /* Large picture of the screen: on a wide window the sidecard widens, so
     On air, top right, shows the screen big. */
  @media (min-width: 1200px) {
    .sidecard.large-preview {
      flex-basis: 32rem;
    }
  }
  /* Narrower: the whole column slims, so the wall gets the room. Everything
     keeps its place: Next above Previous, then the tiles two to a row. */
  .sidecard.narrow {
    flex-basis: 15rem;
  }
  .sidecard.narrow .transport {
    grid-template-columns: 1fr;
  }
  .sidecard.narrow .transport :global(button:last-child) {
    order: -1;
  }
  .sidecard.narrow .ready-screens,
  .sidecard.narrow .ready-tiles {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
  @media (prefers-reduced-motion: reduce) {
    .sidecard {
      transition: none;
    }
  }
  .sidecard .no-screen {
    margin-bottom: 0;
  }
  .sidecard :global(.sound) {
    margin: 0;
    background: var(--panel);
    border-color: transparent;
  }
  .transport {
    display: grid;
    grid-template-columns: 1fr 1.6fr;
    gap: 0.4rem;
  }
  .transport :global(button) {
    height: 2.75rem;
    justify-content: center;
    padding: 0 0.5rem;
  }
  .side-hint {
    margin-left: auto;
    color: var(--muted);
  }
  .ready-screens {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 0.4rem;
  }
  /* Clear text: the background with no words, drawn as a plain dark panel. */
  .clear-pic {
    display: block;
    aspect-ratio: 16 / 9;
    border-radius: 2px;
    background: linear-gradient(160deg, #1d2740, #10151f);
  }
  .screen-pick {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    min-width: 0;
    padding: 0.2rem;
    font: inherit;
    font-size: 0.8rem;
    text-align: left;
    color: var(--text);
    background: none;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    cursor: pointer;
  }
  .screen-pick:hover {
    border-color: var(--gold-hover);
  }
  .screen-pick.on {
    border-color: var(--gold);
    box-shadow: 0 0 0 1px var(--gold);
  }
  .screen-pick.blank.on {
    border-color: var(--live);
    box-shadow: 0 0 0 1px var(--live);
  }
  .screen-caption {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    min-width: 0;
    padding: 0 0.15rem;
  }
  .screen-caption strong {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .screen-caption kbd {
    margin-left: auto;
    font: inherit;
    font-size: 0.7em;
    padding: 0 0.3rem;
    color: var(--muted);
    border: 1px solid currentColor;
    border-radius: 3px;
  }
  .screen-what {
    margin-left: auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.72rem;
    color: var(--muted);
  }
  .ready-tiles {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 0.4rem;
  }
  .ready-tile {
    position: relative;
    min-width: 0;
  }
  .ready-pick {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    width: 100%;
    padding: 0.2rem;
    font: inherit;
    font-size: 0.78rem;
    text-align: left;
    color: var(--text);
    background: none;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    cursor: pointer;
  }
  .ready-pick:hover {
    border-color: var(--gold-hover);
  }
  .ready-tile.on-air .ready-pick {
    border-color: var(--live);
  }
  .ready-caption {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    min-width: 0;
    padding: 0 0.15rem;
  }
  .ready-remove.on-tile {
    position: absolute;
    top: 0.35rem;
    right: 0.35rem;
    width: 1.4rem;
    height: 1.4rem;
    background: rgb(0 0 0 / 0.6);
    color: var(--text);
    opacity: 0;
  }
  .ready-tile:hover .ready-remove.on-tile,
  .ready-remove.on-tile:focus-visible {
    opacity: 1;
  }
  @media (hover: none) {
    .ready-remove.on-tile {
      opacity: 1;
    }
  }
  .ready-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .ready-row {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    border-radius: var(--radius);
  }
  .ready-row.on-air {
    background: var(--hover, rgb(255 255 255 / 0.07));
  }
  .ready-show {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 0.5rem;
    min-height: 2.5rem;
    padding: 0 0.4rem;
    font: inherit;
    font-size: 0.875rem;
    text-align: left;
    color: var(--text);
    background: none;
    border: none;
    border-radius: var(--radius);
    cursor: pointer;
  }
  .ready-show:hover {
    color: var(--gold-hover);
  }
  .ready-show :global(.icon) {
    flex-shrink: 0;
    color: var(--muted);
  }
  .ready-title {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ready-show .side-hint {
    font-size: 0.75rem;
  }
  .ready-remove {
    flex-shrink: 0;
    display: grid;
    place-items: center;
    width: 1.75rem;
    height: 1.75rem;
    padding: 0;
    color: var(--muted);
    background: none;
    border: none;
    border-radius: var(--radius);
    cursor: pointer;
  }
  .ready-remove:hover {
    color: var(--text);
  }
  .wall-back {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    font-size: 0.8125rem;
  }
  .slide-cell {
    position: relative;
  }
  .keep-slide {
    position: absolute;
    top: 0.35rem;
    right: 0.35rem;
    display: inline-flex;
    align-items: center;
    gap: 0.2rem;
    padding: 0.05rem 0.4rem;
    font: inherit;
    font-size: 0.7rem;
    color: #fff;
    background: rgb(0 0 0 / 0.7);
    border: 1px solid rgb(255 255 255 / 0.25);
    border-radius: 999px;
    cursor: pointer;
  }
  .keep-slide.on {
    color: var(--gold-hover);
    border-color: currentColor;
  }
  @media (hover: hover) {
    .keep-slide:not(.on) {
      opacity: 0;
    }
    .slide-cell:hover .keep-slide,
    .keep-slide:focus-visible {
      opacity: 1;
    }
  }
  .wall-count {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.4rem;
  }
  .pin {
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
    padding: 0.05rem 0.4rem;
    font: inherit;
    font-size: 0.72rem;
    color: var(--muted);
    background: none;
    border: 1px solid var(--line);
    border-radius: 999px;
    cursor: pointer;
  }
  .pin:hover {
    color: var(--text);
  }
  .pin.on {
    color: var(--gold-hover);
    border-color: currentColor;
  }
  /* With a mouse, "Keep ready" waits for the row to be pointed at, so the
     wall stays quiet; a kept one always shows. */
  @media (hover: hover) {
    .pin:not(.on) {
      opacity: 0;
    }
    .wall-row:hover .pin,
    .pin:focus-visible {
      opacity: 1;
    }
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
      max-height: max(45%, 20rem);
      padding: 0.75rem 1rem;
      border-bottom: 1px solid var(--line);
      flex-direction: row;
      overflow: hidden;
    }
    .col-onair,
    .col-press {
      flex: 1 1 0;
      min-width: 0;
      min-height: 0;
      max-height: none;
      overflow-y: auto;
      border: none;
      padding: 0 0.5rem;
    }
    .sidecard .monitor,
    .sidecard .wall-steps figure {
      max-width: 12rem;
    }
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
  .wall-play {
    font: inherit;
    font-size: 0.8125rem;
    font-weight: 600;
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    margin-left: 0.4rem;
    padding: 0.2rem 0.65rem;
    color: var(--text);
    background: var(--raised);
    border: 1px solid var(--gold);
    border-radius: 999px;
    cursor: pointer;
  }
  .wall-play:hover {
    background: var(--gold-soft);
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
  /* Each item: its name on one line, its slides across the full width
     beneath, so no column of white space sits beside the slides. */
  .wall-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 0.45rem;
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
    align-items: baseline;
    gap: 0.5rem;
    min-width: 0;
  }
  .wall-title {
    min-width: 0;
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    column-gap: 0.75rem;
    row-gap: 0.25rem;
  }
  .wall-tip {
    max-width: 34rem;
  }
  .wall-title .wall-notes {
    flex-basis: 100%;
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
    color: var(--live-text);
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
