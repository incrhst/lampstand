<!-- Operator panel: the running order on the left, what is on air and
     the slides of the item in view in the middle, Next and notes on the
     right. While a deck is on air the middle is its presenter view
     (current slide at its build step, next click, speaker notes, timer).
     Where OBS matters, the bar has one OBS control (the scene on air and
     the next item's scene) that opens the OBS drawer (T91); OBS's
     connection status is in the footer.
     T47 replaces this with the full operator view. -->
<script lang="ts">
  import { tick, untrack } from "svelte";
  import { request, send, type Command, type Cursor, type DisplayMatch, type OutputConfig, type Service, type ServiceSummary, type SlideView } from "../../lib/api";
  import { isCalibrated, live, mainOutput, pictures, refreshPictures, report } from "../../lib/state.svelte";
  import { sectionCursor } from "../../lib/sections";
  import { neighbourItem, nextClick, nextSlide } from "../../lib/cursor";
  import Button from "../../lib/kit/Button.svelte";
  import Dot from "../../lib/kit/Dot.svelte";
  import Icon from "../../lib/kit/Icon.svelte";
  import ListRow from "../../lib/kit/ListRow.svelte";
  import Placeholder from "../../lib/kit/Placeholder.svelte";
  import Segmented from "../../lib/kit/Segmented.svelte";
  import SlidePreview from "../../lib/kit/SlidePreview.svelte";
  import StatusPill from "../../lib/kit/StatusPill.svelte";
  import { kindIcon, kindLabel, kindName } from "../../lib/kit/items";
  import PresenterView from "./PresenterView.svelte";
  import ObsDrawer from "./ObsDrawer.svelte";
  import QuickAdd from "./QuickAdd.svelte";
  import { loadMediaThumb } from "../../lib/mediaThumbLoader";
  import ObsNotice from "./ObsNotice.svelte";
  import { isConnected, isPicture, isScreen, obsPill, pictureStatus, upcomingScene } from "../../lib/obs";
  import Banner from "../../lib/kit/Banner.svelte";
  import { formatWhen } from "../../lib/when";
  import { newScreen, screenOffers, type ScreenOffer } from "../../lib/screenOffers";
  import { steppedDown, windowToDemote } from "../../lib/primary";

  /** Opens Setup › Screens (the no-screen prompt), Setup › OBS and Plan (the blank view). */
  let { onsetup, onobs, onplan }: { onsetup?: () => void; onobs?: () => void; onplan?: () => void } = $props();

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
  $effect(() => {
    if (live.loaded) return;
    request({ area: "Service", request: "List" }, "Services")
      .then((all) => (services = all.slice(0, OFFERED)))
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
  let screens = $derived(live.outputs.filter(isScreen));
  let sentToObs = $derived(live.outputs.filter(isPicture));

  // Whether OBS has loaded each picture, checked every few seconds while
  // there is one.
  $effect(() => {
    if (sentToObs.length === 0) return;
    refreshPictures();
    const timer = setInterval(refreshPictures, 3000);
    return () => clearInterval(timer);
  });

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
    if (live.loaded && e.key === "/") {
      e.preventDefault();
      quickAddOpen = true;
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
  function quickAdded(id: string | null, showNow: boolean, title: string) {
    quickAddOpen = false;
    live.status = `Added ${title}`;
    if (showNow && id) show({ item: id, slide: 0, step: 0 });
  }
  let viewed = $derived(items.find((i) => i.id === viewing) ?? items.find((i) => i.slides.length > 0) ?? items[0]);
  let liveIndex = $derived(items.findIndex((i) => i.id === live.live?.item));
  let liveItem = $derived(liveIndex >= 0 ? items[liveIndex] : null);
  let upcoming = $derived(live.loaded ? nextSlide(live.loaded, live.live) : null);
  // A deck on air gets the presenter view, whose Next is the next click.
  let presenting = $derived(!!(liveItem && live.live && live.liveSlide && isDeckSlide(live.liveSlide)));
  let clickNext = $derived(live.loaded ? nextClick(live.loaded, live.live) : null);
  // The picture of what is on air is large for a deck (the presenter view)
  // and for an image or video, which fills the big preview.
  let liveWidth = $derived(presenting || mediaOf(liveItem?.id) ? 1280 : 480);

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
    if (liveItem && live.live && live.liveSlide && hasPicture(liveItem.id, live.liveSlide)) {
      want.push([liveItem.id, live.live.slide, live.live.step, liveWidth]);
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
      const s = await request({ area: "Settings", request: { Put: { clicker_mode: !clicker } } }, "Settings");
      clicker = s.clicker_mode;
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

{#if !live.loaded}
  <div class="none">
    {#if screensOn === 0}<div class="no-screen">{@render noScreen()}</div>{/if}
    {#if pill?.tone === "warn" || live.obs?.held || live.obs?.scene_problem}<div class="no-screen obs-notice"><ObsNotice view={live.obs} onsetup={onobs} /></div>{/if}
    <div class="pick">
      <div class="pick-head">
        <h1>No service is open</h1>
        <p class="muted">Open one to go live, or plan a new one.</p>
      </div>
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
                  <Button variant="primary" size="small" icon="play" onclick={() => openService(svc.id)} label="Open {svc.title} for live">
                    Open
                  </Button>
                {/if}
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
  <div class="operate">
    <div class="bar">
      <p class="service">
        <strong title={live.loaded.title}>{live.loaded.title}</strong>
        <!-- Most services share a title ("Sunday service"), so the date tells them apart. -->
        {#if service?.starts_at != null}<span class="when">· {formatWhen(service.starts_at, service.time_zone)}</span>{/if}
      </p>
      <Placeholder ticket="T36">Readiness</Placeholder>
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
      <span class="keys faint"><Icon name="keyboard" size={14} />← → or Space to advance · . blank · 1–9 verse · C B P T I E sections · ⇧↑ ⇧↓ choose an item, Enter starts it · / quick add{pill ? " · O control OBS" : ""}</span>
      <Button size="small" icon="safeArea" pressed={showGuide} onclick={() => (showGuide = !showGuide)}>Safe area</Button>
      <Button size="small" icon="keyboard" pressed={clicker} title="Clicker mode: Page Up, Page Down, B (blank) and full stop work in any app while this service is open" onclick={toggleClicker}>Clicker</Button>
      <Segmented options={modes} value={live.overlay} onchange={setOverlay} label="Screen" />
      <Button size="small" onclick={() => command("ClearText")} title="Take the words off, keep the background">Clear text</Button>
    </div>

    <!-- While a deck is on air the presenter view takes the right column
         too; outputs stay in the status bar. -->
    <div class="grid" class:presenting>
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
          <Button size="small" icon="plus" shortcut="/" onclick={() => (quickAddOpen = true)} title="Quick add a song or scripture after what is on air">Quick add</Button>
        </div>
        <ol class="plain pane-body">
          {#each items as item, i (item.id)}
            {@const kind = kindOf(item.id)}
            <ListRow
              title={item.title}
              subtitle={item.slides.length === 0 ? "Nothing to show" : `${item.slides.length} ${item.slides.length === 1 ? "slide" : "slides"}`}
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
        </ol>
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
                <span class="muted">· {liveItem.title} · {live.liveSlide?.label ?? live.live.slide + 1}</span>
            {/if}
          </div>
          <div class="live-screen" class:on={live.overlay === "None" && !!live.live} aria-live="polite">
            <SlidePreview
              lines={live.liveSlide?.lines ?? []}
              image={live.live ? thumbFor(liveItem?.id, live.live.slide, live.live.step, liveWidth) : null}
              overlay={live.overlay}
              safeArea={output?.safe_area}
              {showGuide}
              size="large"
              empty="Nothing on air. Press Space to start."
            />
          </div>
          <div class="transport">
            <Button icon="prev" onclick={() => command("Previous")} shortcut="←">Previous</Button>
            <Button variant="primary" onclick={() => command("Next")} shortcut="Space">Next</Button>
            <Button variant="danger" onclick={() => command("ToggleBlack")} pressed={live.overlay === "Black"} shortcut=".">Blank</Button>
            <Button variant="ghost" onclick={() => command("ClearAll")} title="Clear text, background and overlay">Clear all</Button>
          </div>
          {/if}

          {#if viewed}
            <div class="slides-head">
              <h2 class="title">{viewed.title}</h2>
              <span class="faint">Click a slide to take it live</span>
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
                    <SlidePreview lines={slide.lines} image={builtThumb(viewed.id, i)} size="small" />
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
                  image={builtThumb(upcoming.cursor.item, upcoming.cursor.slide)}
                  safeArea={output?.safe_area}
                />
              {:else}
                <SlidePreview empty="End of service" />
              {/if}
            </div>
          </section>

          <section>
            <h2 class="title">Notes</h2>
            {#if notes}
              <p class="notes">{notes}</p>
            {:else}
              <p class="faint small">No planner notes on this item.</p>
            {/if}
          </section>

          <section>
            <h2 class="title">Screens</h2>
            <ul class="plain outputs">
              {#each screens as o (o.id)}
                <li>
                  <Dot tone={o.enabled && live.overlay === "None" && live.live ? "live" : "neutral"} />
                  <span class="name">{o.name}{#if !isCalibrated(o)}<span class="warn"> · not calibrated</span>{/if}</span>
                  <span class="muted small">{!o.enabled ? "Off" : live.overlay === "None" && !live.live ? "Nothing on air" : outputState[live.overlay]}</span>
                </li>
              {:else}
                <li class="faint small">No screens. Add one in Setup › Screens.</li>
              {/each}
            </ul>
          </section>

          {#if sentToObs.length > 0}
            <section>
              <h2 class="title">Sent to OBS</h2>
              <ul class="plain outputs">
                {#each sentToObs as o (o.id)}
                  {@const status = pictureStatus(o.enabled, pictures.clients[o.id] ?? 0)}
                  <li title={status.title}>
                    <Icon name="broadcast" size={14} />
                    <span class="name">{o.name}</span>
                    <span class="muted small status-{status.tone}">{status.text}</span>
                  </li>
                {/each}
              </ul>
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
    .keys {
      display: none;
    }
  }
  /* A long title truncates and the date keeps its width, so the controls
     stay on one row in the same place whatever the service is called. */
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
  /* The hint gives way first: it takes the room left over and is cut
     from the end with an ellipsis. */
  .keys {
    flex: 1 1 0;
    min-width: 0;
    text-align: right;
    font-size: 0.75rem;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .keys :global(.icon) {
    vertical-align: -2px;
    margin-right: 0.4rem;
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
  .live-screen {
    border-radius: var(--radius-lg);
    overflow: hidden;
    box-shadow: 0 0 0 2px var(--line);
  }
  .live-screen.on {
    box-shadow: 0 0 0 2px var(--live);
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
  .small {
    font-size: 0.8rem;
    margin-top: 0.5rem;
  }
  .outputs {
    margin-top: 0.6rem;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  .outputs li {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.85rem;
  }
  .outputs .status-ready {
    color: var(--ready, var(--text));
  }
  .outputs .status-warn {
    color: var(--gold);
  }
  .outputs .small {
    margin: 0 0 0 auto;
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .warn {
    color: var(--warn);
    font-size: 0.75rem;
  }
</style>
