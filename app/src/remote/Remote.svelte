<!-- Lampstand Remote (T100, the wall in 0.9.1), phones and tablets. A
     phone held upright, from the top: a strip with a small picture of the
     main screen and LIVE, IDLE or BLANK with what is on air; the whole
     service in the middle, each item with its slides as cards that swipe
     sideways (LIVE and NEXT marked as on the laptop's wall); and at the foot,
     in thumb reach, big Back and Next, then Blank, Idle and "Stop the sound
     now". The Sound box folds out above that bar. A card takes two taps: the
     first selects it and the bar offers "Show this slide"; nothing goes on
     air by accident. On a tablet, or a phone on its side, the rows become the
     desktop wall (cards in a wrapping grid) with the controls along the top.
     Nothing to plan or edit: Plan stays on the laptop. When the link drops,
     everything the laptop said goes and the page says it is not connected. -->
<script lang="ts">
  import type { Command, Cursor, Request, SoundRequest } from "../lib/api";
  import { onMount, tick } from "svelte";
  import { words as soundWords } from "../lib/audio.ts";
  import Dot from "../lib/kit/Dot.svelte";
  import Icon from "../lib/kit/Icon.svelte";
  import SlidePreview from "../lib/kit/SlidePreview.svelte";
  import StatusPill from "../lib/kit/StatusPill.svelte";
  import { onScreen, pictureKey, remoteSounds, upNext, wantsPicture, words } from "../lib/remote.ts";
  import {
    PICTURE_WIDTH,
    PictureQueue,
    WIDE_FROM,
    keepSelection,
    layoutFor,
    liveKey,
    nextLabel,
    selectionLine,
    serviceShape,
    status,
    tap,
    twoStepTap,
    wallGroups,
    wallWords,
    type WallCard,
  } from "../lib/remoteWall.ts";
  import { link } from "./link.svelte";
  import SoundBox from "./SoundBox.svelte";
  import { keepAwake } from "./wake";

  let online = $derived(link.state === "online");
  let loaded = $derived(link.loaded);
  let shown = $derived(onScreen(link.loaded, link.live));
  let coming = $derived(upNext(link.loaded, link.live));
  let overlay = $derived(link.live?.overlay ?? "None");
  let groups = $derived(wallGroups(link.loaded, link.live));
  let said = $derived(status(link.loaded, link.live));
  let nextText = $derived(nextLabel(link.loaded, link.live));

  // Upright phone or wide wall, by width; touch keeps the two-step tap.
  let width = $state(typeof window === "undefined" ? 390 : window.innerWidth);
  let coarse = $state(false);
  onMount(() => {
    const wide = matchMedia(`(min-width: ${WIDE_FROM}px)`);
    const touch = matchMedia("(pointer: coarse)");
    const update = () => {
      width = window.innerWidth;
      coarse = touch.matches;
    };
    update();
    wide.addEventListener("change", update);
    touch.addEventListener("change", update);
    return () => {
      wide.removeEventListener("change", update);
      touch.removeEventListener("change", update);
    };
  });
  let layout = $derived(layoutFor(width));
  let twoStep = $derived(twoStepTap(coarse, layout));

  // The Sound box folds out; "Stop the sound now" is always in the bar.
  let sounds = $derived(remoteSounds(link.loaded));
  let playing = $derived(link.sound?.playing ?? null);
  let hasSound = $derived(online && (sounds.length > 0 || playing != null));
  let soundOpen = $state(false);
  let soundLine = $derived(
    playing ? `${playing.paused ? soundWords.paused : playing.ending ? soundWords.fading : soundWords.playing} · ${playing.name}` : soundWords.quiet,
  );

  // The two-step tap: the card selected, waiting for "Show this slide".
  let selected: Cursor | null = $state(null);
  let selectedLine = $derived(selectionLine(link.loaded, selected));
  // Any live change (Next, Back, Blank, Idle, the laptop, the link dropping)
  // cancels the selection; so does a reload that takes its slide away.
  let seenLive = "";
  $effect(() => {
    const key = liveKey(link.live);
    if (key !== seenLive) {
      seenLive = key;
      selected = null;
    }
  });
  $effect(() => {
    const kept = keepSelection(link.loaded, selected);
    if (kept !== selected) selected = kept;
  });

  // Slide pictures: deck slides and pictures, drawn by the laptop at 240 px,
  // asked for two at a time as their rows come into view, the slide on air
  // first, and kept while the service stays the same.
  let pictures: Record<string, string> = $state({});
  const queue = new PictureQueue<Cursor>(
    async (cursor) => {
      const r = await link.send({
        area: "Service",
        request: { SlideThumbnail: { item: cursor.item, slide: cursor.slide, step: cursor.step, width: PICTURE_WIDTH } },
      });
      return r.type === "Thumbnail" ? r.data : null;
    },
    (key, picture) => (pictures[key] = picture),
  );
  const keyOf = (cursor: Cursor) => (loaded ? pictureKey(loaded, cursor) : "");
  const pictureAt = (cursor: Cursor | undefined) => (loaded && cursor ? (pictures[pictureKey(loaded, cursor)] ?? null) : null);
  /** Cards show a deck slide fully built, as on the laptop. */
  const built = (card: WallCard): Cursor => ({ ...card.cursor, step: card.slide.steps });
  let shape = "";
  $effect(() => {
    const now = serviceShape(link.loaded);
    if (now !== shape) {
      // A reconnect says the same service again: its pictures stay.
      if (now !== "" || !online) {
        shape = now;
        if (now !== "") {
          queue.reset();
          pictures = {};
        }
      }
    }
  });
  // In view: the cards on screen now, asked for again after a reconnect.
  const inView = new Map<string, Cursor>();
  let wasOnline = false;
  $effect(() => {
    if (online && !wasOnline) {
      queue.retry();
      for (const [key, cursor] of inView) queue.want(key, cursor, 2);
    }
    wasOnline = online;
  });
  $effect(() => {
    if (!online || !loaded) return;
    // Most urgent: what the screens show, then what Next shows.
    if (shown && wantsPicture(shown.slide)) queue.want(keyOf(shown.cursor), shown.cursor, 0);
    if (coming && wantsPicture(coming.slide)) queue.want(keyOf(coming.cursor), coming.cursor, 1);
  });

  // Made before the cards, so each card's action can hand itself over.
  function seen(entries: IntersectionObserverEntry[]) {
    for (const e of entries) {
      const el = e.target as HTMLElement;
      const cursor: Cursor = { item: el.dataset.item!, slide: Number(el.dataset.slide), step: Number(el.dataset.step) };
      const key = keyOf(cursor);
      if (!key) continue;
      if (e.isIntersecting) {
        inView.set(key, cursor);
        if (online) queue.want(key, cursor, 2);
      } else {
        inView.delete(key);
        queue.drop(key);
      }
    }
  }
  const observer = typeof IntersectionObserver === "undefined" ? null : new IntersectionObserver(seen, { rootMargin: "200px" });
  onMount(() => () => observer?.disconnect());

  /** Watches a card that needs a picture, so it is asked for once it is near the screen. */
  function watchIf(node: HTMLElement, on: boolean) {
    let watching = false;
    const set = (want: boolean) => {
      if (want && !watching) observer?.observe(node);
      if (!want && watching) observer?.unobserve(node);
      watching = want && !!observer;
    };
    set(on);
    return { update: set, destroy: () => set(false) };
  }

  // The row on air comes into view when the service moves to another item,
  // and its strip slides to the live card.
  let wall: HTMLElement | undefined = $state();
  let lastItem: string | null = null;
  $effect(() => {
    const cursor = link.live?.live;
    void layout;
    if (!cursor || !wall) return;
    const moved = cursor.item !== lastItem;
    lastItem = cursor.item;
    void tick().then(() => {
      const row = wall?.querySelector<HTMLElement>(".row.on-air");
      if (!row) return;
      if (moved) row.scrollIntoView({ block: "nearest", behavior: "smooth" });
      const strip = row.querySelector<HTMLElement>(".strip");
      const card = row.querySelector<HTMLElement>(".card.live")?.parentElement;
      if (strip && card && strip.scrollWidth > strip.clientWidth) {
        strip.scrollTo({ left: card.offsetLeft - (strip.clientWidth - card.offsetWidth) / 2, behavior: "smooth" });
      }
    });
  });

  /** What the laptop refused or could not do, in its words, for a few seconds. */
  let problem: string | null = $state(null);
  let problemTimer: ReturnType<typeof setTimeout> | undefined;
  async function act(request: Request) {
    keepAwake();
    navigator.vibrate?.(8);
    problem = null;
    clearTimeout(problemTimer);
    try {
      await link.send(request);
    } catch (e) {
      problem = e instanceof Error ? e.message : String(e);
      problemTimer = setTimeout(() => (problem = null), 6000);
    }
  }
  const live = (command: Command) => {
    selected = null;
    void act({ area: "Live", request: command });
  };
  const sound = (request: SoundRequest) => act({ area: "Sound", request });
  const show = (cursor: Cursor) => live({ Show: cursor });

  function tapCard(cursor: Cursor) {
    keepAwake();
    if (!twoStep) return show(cursor);
    const r = tap(selected, cursor);
    if (r.show) show(r.show);
    else selected = r.selected;
  }
  const isSelected = (cursor: Cursor) => selected?.item === cursor.item && selected.slide === cursor.slide;
  let disabled = $derived(!online || !loaded);
</script>

{#snippet monitor()}
  <div class="monitor {said.tone}" role="img" aria-label="What the main screen shows">
    <SlidePreview
      lines={shown && overlay === "None" ? shown.slide.lines : []}
      footer={shown && overlay === "None" ? (shown.slide.footer ?? null) : null}
      image={shown && overlay === "None" && wantsPicture(shown.slide) ? pictureAt(shown.cursor) : null}
      overlay={said.tone === "idle" ? "Idle" : overlay}
      size="small"
      empty={shown?.slide.label ?? ""}
    />
  </div>
{/snippet}

{#snippet statusText()}
  <div class="said" aria-live="polite">
    <span class="state {said.tone}"><span class="dot" aria-hidden="true"></span>{said.label}</span>
    {#if said.item}<strong class="item">{said.item}</strong>{/if}
    <span class="detail">{said.detail}</span>
  </div>
{/snippet}

{#snippet linkLine()}
  <span class="link" role="status">
    <Dot tone={online ? "ready" : "warn"} size={7} />
    {online ? words.online : link.state === "connecting" ? words.connecting : words.offline}
    {#if loaded}<span class="service" title={loaded.title}>· {loaded.title}</span>{/if}
  </span>
{/snippet}

{#snippet panic()}
  <button type="button" class="panic" disabled={!online} onclick={() => sound("Panic")}>{soundWords.panic}</button>
{/snippet}

{#snippet soundHandle()}
  {#if hasSound}
    <button type="button" class="sound-handle" class:playing={!!playing} aria-expanded={soundOpen} aria-controls="sound-box" onclick={() => (soundOpen = !soundOpen)}>
      <Icon name="sound" size={18} />
      <span class="sound-line">{soundWords.title} · {soundLine}</span>
      <span class="chevron" class:open={soundOpen}><Icon name="chevronDown" size={16} /></span>
    </button>
  {/if}
{/snippet}

{#snippet soundSheet()}
  {#if hasSound && soundOpen}
    <div class="sheet" id="sound-box"><SoundBox onsound={sound} /></div>
  {/if}
{/snippet}

{#snippet selection()}
  <div class="selection" role="region" aria-label={wallWords.selected}>
    <p class="selected-line">{wallWords.selected} <strong>{selectedLine}</strong></p>
    <div class="selected-buttons">
      <button type="button" class="cancel" onclick={() => (selected = null)}>{wallWords.cancel}</button>
      <button type="button" class="go" disabled={!online} onclick={() => selected && show(selected)}>{wallWords.showThis}</button>
    </div>
  </div>
{/snippet}

{#snippet card(c: WallCard, title: string)}
  {@const picture = c.picture ? built(c) : null}
  <button
    type="button"
    class="card"
    class:live={c.mark === "live"}
    class:next={c.mark === "next"}
    class:selected={isSelected(c.cursor)}
    aria-current={c.mark === "live" ? "true" : undefined}
    aria-pressed={twoStep ? isSelected(c.cursor) : undefined}
    aria-label="{title} · {c.label}"
    data-item={picture?.item}
    data-slide={picture?.slide}
    data-step={picture?.step}
    use:watchIf={!!picture}
    {disabled}
    onclick={() => tapCard(c.cursor)}
  >
    <SlidePreview lines={c.slide.lines} footer={c.slide.footer} image={picture ? pictureAt(picture) : null} size="small" empty={c.label} />
    <span class="caption">
      {#if c.mark === "live"}<StatusPill tone="live">LIVE</StatusPill>{:else if c.mark === "next"}<StatusPill tone="gold">NEXT</StatusPill>{/if}
      <span class="label">{c.label}</span>
      {#if c.slide.steps > 0}<span class="builds">{wallWords.builds(c.slide.steps)}</span>{/if}
    </span>
  </button>
{/snippet}

{#snippet theWall()}
  <div class="wall" bind:this={wall} aria-label={wallWords.service}>
    {#if !online}
      <div class="blank">{words.offline}</div>
    {:else if !loaded}
      <div class="blank">{words.nothingLoaded}</div>
    {:else}
      {#each groups as group (group.heading?.header ?? "top")}
        <section class="group" aria-label={group.heading?.title}>
          {#if group.heading}<h2 class="heading">{group.heading.title}</h2>{/if}
          {#each group.rows as row (row.item.id)}
            <section class="row" class:on-air={row.onAir} class:played={row.played} aria-label={row.item.title}>
              <div class="row-label">
                <span class="num" class:current={row.onAir}>{row.number}</span>
                <div class="row-title">
                  <h3>{row.item.title}</h3>
                  <p class:on-air-tag={row.onAir}>{#if row.onAir}<Dot tone="live" size={6} />{/if}{row.subtitle}</p>
                </div>
              </div>
              {#if row.cards.length > 0}
                <ul class="plain strip">
                  {#each row.cards as c (c.cursor.slide)}
                    <li>{@render card(c, row.item.title)}</li>
                  {/each}
                </ul>
              {:else if row.sound}
                <div class="sound-card" class:on-air={row.onAir}>
                  <Icon name="sound" size={18} />
                  <span class="sound-name">{row.sound.name}</span>
                  <span class="muted">{row.sound.start} · {row.sound.length}</span>
                </div>
              {/if}
            </section>
          {/each}
        </section>
      {/each}
    {/if}
  </div>
{/snippet}

<main class="remote {layout}" class:offline={!online}>
  {#if layout === "narrow"}
    <header class="top">
      {@render linkLine()}
      <div class="now">
        {@render monitor()}
        {@render statusText()}
      </div>
    </header>
    {#if !online && link.state === "offline"}<p class="banner" role="alert">{words.offlineLong}</p>{/if}
    {@render theWall()}
    <footer class="bottom">
      <p class="problem" role="alert">{problem ?? ""}</p>
      {@render soundSheet()}
      {@render soundHandle()}
      {#if selected}
        {@render selection()}
      {:else}
        <div class="drive">
          <button type="button" class="back" {disabled} onclick={() => live("Previous")}><Icon name="prev" size={24} />{wallWords.back}</button>
          <button type="button" class="go" {disabled} onclick={() => live("Next")}>{nextText}<Icon name="next" size={24} /></button>
        </div>
      {/if}
      <div class="quiet">
        <button type="button" disabled={!online} aria-pressed={overlay === "Black"} onclick={() => live("ToggleBlack")}>{wallWords.blankButton}</button>
        <button type="button" disabled={!online} aria-pressed={overlay === "Idle"} onclick={() => live("ToggleIdle")}>{wallWords.idleButton}</button>
        {@render panic()}
      </div>
    </footer>
  {:else}
    <header class="top">
      <div class="now">
        {@render monitor()}
        <div class="now-text">
          {@render statusText()}
          {@render linkLine()}
        </div>
      </div>
      <div class="controls">
        <button type="button" class="back" {disabled} onclick={() => live("Previous")}><Icon name="prev" size={20} />{wallWords.previous}</button>
        <button type="button" class="go" {disabled} onclick={() => live("Next")}>{nextText}<Icon name="next" size={20} /></button>
        <button type="button" class="quiet-button" disabled={!online} aria-pressed={overlay === "Black"} onclick={() => live("ToggleBlack")}>{wallWords.blankButton}</button>
        <button type="button" class="quiet-button" disabled={!online} aria-pressed={overlay === "Idle"} onclick={() => live("ToggleIdle")}>{wallWords.idleButton}</button>
        <button type="button" class="quiet-button" disabled={!online} title={words.clearHint} onclick={() => live("ClearText")}>{wallWords.clear}</button>
        {@render panic()}
      </div>
    </header>
    {#if !online && link.state === "offline"}<p class="banner" role="alert">{words.offlineLong}</p>{/if}
    <p class="problem" role="alert">{problem ?? ""}</p>
    {#if hasSound}
      <div class="sound-row">{@render soundHandle()}</div>
      {@render soundSheet()}
    {/if}
    {#if selected}{@render selection()}{/if}
    {@render theWall()}
  {/if}
</main>

<style>
  .remote {
    --idle: var(--role-projection);
    box-sizing: border-box;
    height: 100dvh;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    -webkit-tap-highlight-color: transparent;
  }
  .remote :global(button) {
    font: inherit;
    color: var(--text);
    background: var(--raised);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    min-height: max(44px, 2.75rem);
    padding: 0 0.9rem;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 0.5rem;
    cursor: pointer;
    touch-action: manipulation;
    user-select: none;
  }
  .remote :global(button:active:not(:disabled)) {
    background: var(--line-strong);
  }
  .remote :global(button:disabled) {
    opacity: 0.45;
    filter: saturate(0);
  }

  /* The top strip: always there. */
  .top {
    flex-shrink: 0;
    padding: max(0.5rem, env(safe-area-inset-top)) max(0.75rem, env(safe-area-inset-right)) 0.6rem max(0.75rem, env(safe-area-inset-left));
    border-bottom: 1px solid var(--line);
    background: var(--canvas);
    display: grid;
    gap: 0.4rem;
  }
  .link {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    min-width: 0;
    font-size: 0.75rem;
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
  }
  .link .service {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .now {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    min-width: 0;
  }
  .monitor {
    flex: 0 0 104px;
    width: 104px;
    border-radius: var(--radius);
    overflow: hidden;
    box-shadow: 0 0 0 2px var(--line);
  }
  .monitor.live {
    box-shadow: 0 0 0 2px var(--live);
  }
  .offline .monitor {
    box-shadow: 0 0 0 2px var(--line);
  }
  .said {
    display: grid;
    gap: 0.1rem;
    min-width: 0;
    font-size: 0.85rem;
  }
  .said > * {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .state {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    font-size: 0.7rem;
    font-weight: 700;
    letter-spacing: 0.08em;
  }
  .state .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--faint);
  }
  .state.live {
    color: var(--live);
  }
  .state.live .dot {
    background: var(--live);
  }
  .state.idle {
    color: var(--idle);
  }
  .state.idle .dot {
    background: var(--idle);
  }
  .state.blank {
    color: var(--muted);
  }
  .item {
    font-size: 0.95rem;
  }
  .detail {
    color: var(--muted);
  }
  .banner {
    flex-shrink: 0;
    margin: 0.5rem 0.75rem 0;
    border: 1px solid color-mix(in srgb, var(--warn) 35%, transparent);
    border-radius: var(--radius-lg);
    background: var(--warn-soft);
    color: var(--text);
    padding: 0.6rem 0.8rem;
    font-size: 0.9rem;
  }

  /* The middle: the whole service, scrolling. */
  .wall {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overscroll-behavior: contain;
    padding: 0.5rem max(0.5rem, env(safe-area-inset-right)) 1rem max(0.5rem, env(safe-area-inset-left));
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  .blank {
    margin: 1rem 0.25rem;
    padding: 2rem 1rem;
    text-align: center;
    border: 1px dashed var(--line-strong);
    border-radius: var(--radius);
    color: var(--muted);
  }
  .group {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }
  .heading {
    margin: 0.5rem 0.4rem 0;
    font-size: 0.7rem;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--muted);
  }
  .row {
    padding: 0.5rem 0.4rem;
    border: 1px solid transparent;
    border-radius: var(--radius-lg);
    display: grid;
    gap: 0.4rem;
  }
  .row.on-air {
    background: var(--panel);
    border-color: var(--line-strong);
  }
  .row.played {
    opacity: 0.55;
  }
  .row-label {
    display: flex;
    align-items: baseline;
    gap: 0.55rem;
    min-width: 0;
  }
  .num {
    flex: 0 0 auto;
    min-width: 1.4rem;
    font-size: 0.8rem;
    font-variant-numeric: tabular-nums;
    color: var(--faint);
    text-align: right;
  }
  .num.current {
    color: var(--live);
    font-weight: 700;
  }
  .row-title {
    min-width: 0;
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.5rem;
    flex: 1;
  }
  .row-title h3 {
    font-size: 0.95rem;
    font-weight: 600;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .row-title p {
    flex-shrink: 0;
    font-size: 0.8rem;
    color: var(--muted);
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
  }
  .row-title p.on-air-tag {
    color: var(--live);
    font-weight: 600;
  }
  /* A phone: the cards swipe sideways. */
  .strip {
    position: relative;
    display: flex;
    gap: 0.6rem;
    overflow-x: auto;
    scroll-snap-type: x mandatory;
    scroll-padding: 0 0.25rem;
    padding: 6px 0.4rem 6px;
    scrollbar-width: none;
    overscroll-behavior-x: contain;
  }
  .strip::-webkit-scrollbar {
    display: none;
  }
  .strip > li {
    flex: 0 0 150px;
    scroll-snap-align: start;
  }
  .card {
    width: 100%;
    flex-direction: column;
    align-items: stretch;
    gap: 0;
    padding: 0 !important;
    text-align: left;
    background: var(--panel) !important;
    border: none !important;
    border-radius: var(--radius) !important;
    overflow: hidden;
    box-shadow: 0 0 0 1px var(--line);
  }
  .card.live {
    box-shadow: 0 0 0 2px var(--live);
  }
  .card.next {
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--gold) 75%, transparent);
  }
  /* Selected (the two-step tap): brighter and thicker than NEXT, with a glow. */
  .card.selected {
    outline: 3px solid var(--gold-hover);
    outline-offset: 2px;
    box-shadow: 0 0 16px color-mix(in srgb, var(--gold) 55%, transparent);
  }
  .caption {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.3rem 0.5rem;
    min-height: 1.6rem;
    font-size: 0.75rem;
    color: var(--muted);
  }
  .caption :global(.pill) {
    padding: 0.05rem 0.35rem;
    font-size: 0.6rem;
  }
  .card.live .caption {
    color: var(--text);
    font-weight: 500;
  }
  .label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .builds {
    margin-left: auto;
    flex-shrink: 0;
    color: var(--faint);
  }
  .sound-card {
    margin: 0 0.25rem;
    min-height: max(44px, 2.75rem);
    padding: 0.5rem 0.75rem;
    border: 1px dashed var(--line-strong);
    border-radius: var(--radius);
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.25rem 0.5rem;
    font-size: 0.85rem;
  }
  .sound-card.on-air {
    border-color: var(--live);
  }
  .sound-name {
    font-weight: 600;
  }

  /* The bottom bar, in thumb reach. */
  .bottom {
    flex-shrink: 0;
    padding: 0.5rem max(0.75rem, env(safe-area-inset-right)) max(0.6rem, env(safe-area-inset-bottom)) max(0.75rem, env(safe-area-inset-left));
    border-top: 1px solid var(--line);
    background: var(--canvas);
    display: grid;
    gap: 0.5rem;
  }
  .problem {
    font-size: 0.85rem;
    color: var(--warn);
  }
  .problem:empty {
    display: none;
  }
  .drive {
    display: grid;
    grid-template-columns: 1fr 2fr;
    gap: 0.5rem;
  }
  .drive :global(button) {
    min-height: max(64px, 4rem);
    font-size: 1.2rem;
    font-weight: 600;
  }
  .remote :global(button.go) {
    background: var(--gold);
    border-color: var(--gold);
    color: var(--on-gold);
    font-weight: 600;
  }
  .remote :global(button.go:active:not(:disabled)) {
    background: var(--gold-hover);
  }
  .quiet {
    display: grid;
    grid-template-columns: 1fr 1fr 2fr;
    gap: 0.5rem;
  }
  .quiet :global(button),
  .quiet-button {
    color: var(--muted);
  }
  .remote :global(button[aria-pressed="true"]:not(.card)) {
    background: var(--live-strong);
    border-color: var(--live-strong);
    color: white;
  }
  .panic {
    background: color-mix(in srgb, var(--live-strong) 18%, var(--raised)) !important;
    border-color: color-mix(in srgb, var(--live) 45%, transparent) !important;
    color: var(--text) !important;
    font-weight: 600;
    white-space: nowrap;
    padding: 0 0.6rem !important;
  }
  .panic:active:not(:disabled) {
    background: var(--live-strong) !important;
    color: white !important;
  }
  .selection {
    display: grid;
    gap: 0.5rem;
  }
  .selected-line {
    font-size: 0.9rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .selected-buttons {
    display: grid;
    grid-template-columns: 1fr 2fr;
    gap: 0.5rem;
  }
  .selected-buttons :global(button) {
    min-height: max(64px, 4rem);
    font-size: 1.1rem;
  }
  .sound-handle {
    justify-content: flex-start !important;
    min-height: max(44px, 2.75rem);
    font-size: 0.85rem;
    background: var(--panel) !important;
  }
  .sound-handle.playing {
    border-color: color-mix(in srgb, var(--gold) 50%, transparent);
  }
  .sound-line {
    flex: 1;
    min-width: 0;
    text-align: left;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .chevron {
    display: inline-flex;
    transform: rotate(180deg);
    transition: transform 120ms var(--ease);
  }
  .chevron.open {
    transform: none;
  }
  .sheet {
    max-height: 45dvh;
    overflow-y: auto;
    overscroll-behavior: contain;
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    background: var(--panel);
    padding: 0.75rem;
  }

  /* A tablet, or a phone on its side: the desktop wall, with the controls along the top. */
  .wide .top {
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    gap: 0.75rem;
  }
  .wide .now-text {
    display: grid;
    gap: 0.2rem;
    min-width: 0;
  }
  .wide .monitor {
    flex-basis: 128px;
    width: 128px;
  }
  .controls {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 0.4rem;
  }
  .controls :global(button) {
    min-height: max(48px, 3rem);
  }
  .controls .go {
    min-width: 8rem;
    font-size: 1.05rem;
  }
  .wide .problem,
  .wide .sound-row,
  .wide .sheet,
  .wide .selection {
    flex-shrink: 0;
    margin: 0.5rem max(0.75rem, env(safe-area-inset-right)) 0 max(0.75rem, env(safe-area-inset-left));
  }
  .wide .sound-row {
    display: grid;
  }
  .wide .sheet {
    max-height: 40dvh;
  }
  .wide .selection {
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    padding: 0.5rem 0.75rem;
    border: 1px solid var(--gold);
    border-radius: var(--radius-lg);
    background: var(--gold-soft);
  }
  .wide .selected-buttons {
    grid-template-columns: auto auto;
  }
  .wide .selected-buttons :global(button) {
    min-height: max(48px, 3rem);
  }
  .wide .wall {
    padding: 0.75rem max(1rem, env(safe-area-inset-right)) max(1.5rem, env(safe-area-inset-bottom)) max(1rem, env(safe-area-inset-left));
  }
  .wide .row {
    grid-template-columns: 12rem minmax(0, 1fr);
    gap: 1rem;
    padding: 0.75rem;
    align-items: start;
  }
  .wide .row-title {
    flex-direction: column;
    gap: 0.15rem;
  }
  .wide .row-title h3 {
    white-space: normal;
    overflow-wrap: anywhere;
  }
  .wide .strip {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(170px, 1fr));
    gap: 0.75rem;
    overflow: visible;
    scroll-snap-type: none;
  }
  .wide .sound-card {
    margin: 0;
    max-width: 24rem;
  }
</style>
