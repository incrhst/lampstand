<!-- Operator panel: the running order on the left, what is on air and
     the slides of the item in view in the middle, Next and notes on the
     right. T47 replaces this with the full operator view. -->
<script lang="ts">
  import { untrack } from "svelte";
  import { request, send, type Command, type Cursor, type Service, type SlideView } from "../../lib/api";
  import { isCalibrated, live, mainOutput, report } from "../../lib/state.svelte";
  import { sectionCursor } from "../../lib/sections";
  import { nextSlide } from "../../lib/cursor";
  import Button from "../../lib/kit/Button.svelte";
  import Dot from "../../lib/kit/Dot.svelte";
  import Icon from "../../lib/kit/Icon.svelte";
  import ListRow from "../../lib/kit/ListRow.svelte";
  import Placeholder from "../../lib/kit/Placeholder.svelte";
  import Segmented from "../../lib/kit/Segmented.svelte";
  import SlidePreview from "../../lib/kit/SlidePreview.svelte";
  import StatusPill from "../../lib/kit/StatusPill.svelte";
  import { kindIcon, kindLabel, kindName } from "../../lib/kit/items";

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

  let items = $derived(live.loaded?.items ?? []);
  let viewed = $derived(items.find((i) => i.id === viewing) ?? items.find((i) => i.slides.length > 0) ?? items[0]);
  let liveIndex = $derived(items.findIndex((i) => i.id === live.live?.item));
  let liveItem = $derived(liveIndex >= 0 ? items[liveIndex] : null);
  let upcoming = $derived(live.loaded ? nextSlide(live.loaded, live.live) : null);

  // Deck slides have no lines to draw, so the core renders a picture of
  // each, fetched once per loaded service as slides come into view.
  const THUMB_WIDTH = 480;
  let thumbs: Record<string, string> = $state({});
  const asked = new Set<string>();
  const thumbKey = (item: string, slide: number) => `${item}/${slide}`;
  const isDeckSlide = (slide: SlideView) => slide.lines.length === 0 && /^Slide \d+$/.test(slide.label ?? "");

  async function fetchThumb(item: string, slide: number) {
    const key = thumbKey(item, slide);
    if (asked.has(key)) return;
    asked.add(key);
    try {
      thumbs[key] = await request(
        { area: "Service", request: { SlideThumbnail: { item, slide, width: THUMB_WIDTH } } },
        "Thumbnail",
      );
    } catch {
      // No picture (no GPU, or the deck reloaded): the slide shows its label.
    }
  }

  const thumbFor = (item: string | undefined, slide: number) => (item ? thumbs[thumbKey(item, slide)] : undefined);

  // A reload can change a deck's slides, so pictures are fetched again.
  $effect(() => {
    void live.loaded;
    untrack(() => {
      thumbs = {};
      asked.clear();
    });
  });

  $effect(() => {
    const want: [string, number][] = [];
    if (viewed) viewed.slides.forEach((s, i) => isDeckSlide(s) && want.push([viewed.id, i]));
    if (liveItem && live.live && live.liveSlide && isDeckSlide(live.liveSlide)) want.push([liveItem.id, live.live.slide]);
    if (upcoming && isDeckSlide(upcoming.slide)) want.push([upcoming.cursor.item, upcoming.cursor.slide]);
    untrack(async () => {
      for (const [item, slide] of want) await fetchThumb(item, slide);
    });
  });
  let output = $derived(mainOutput(live.outputs));
  let showGuide = $state(true);

  function kindOf(id: string) {
    const item = service?.items[id];
    return item ? kindName(item.kind) : null;
  }

  let notes = $derived(liveItem ? (service?.items[liveItem.id]?.notes ?? "") : "");

  const stateLabel = { None: "LIVE", Black: "BLANK", Logo: "LOGO" } as const;
  const outputState = { None: "Showing", Black: "Blank", Logo: "Logo" } as const;
</script>

<svelte:window {onkeydown} />

{#if !live.loaded}
  <div class="none">
    <Icon name="play" size={20} />
    <p class="muted">No service is open. Open one from Plan to go live.</p>
  </div>
{:else}
  <div class="operate">
    <div class="bar">
      <p class="service"><strong>{live.loaded.title}</strong></p>
      <Placeholder ticket="T36">Readiness</Placeholder>
      <span class="keys faint"><Icon name="keyboard" size={14} />← → or Space to advance · . blank · 1–9 verse · C B P T I E sections</span>
      <Button size="small" icon="safeArea" pressed={showGuide} onclick={() => (showGuide = !showGuide)}>Safe area</Button>
      <Segmented options={modes} value={live.overlay} onchange={setOverlay} label="Output" />
      <Button size="small" onclick={() => command("ClearText")} title="Take the words off, keep the background">Clear text</Button>
    </div>

    <div class="grid">
      <nav class="pane pane-side queue" aria-label="Service order">
        <div class="pane-head"><h2>Service order</h2></div>
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

      <section class="pane centre" aria-label="Live output">
        <div class="centre-body">
          <div class="live-head">
            <span class="state"><Dot tone={live.overlay === "None" && live.live ? "live" : "neutral"} size={8} />{stateLabel[live.overlay]}</span>
            <span class="muted">{output?.name ?? "No output yet"}</span>
            {#if liveItem && live.live}
              <span class="muted">· {liveItem.title} · {live.liveSlide?.label ?? live.live.slide + 1}</span>
            {/if}
            {#if live.liveSlide?.label === "Video"}
              <Placeholder ticket="T27">Video plays on screen</Placeholder>
            {/if}
          </div>
          <div class="live-screen" class:on={live.overlay === "None" && !!live.live} aria-live="polite">
            <SlidePreview
              lines={live.liveSlide?.lines ?? []}
              image={live.live ? thumbFor(liveItem?.id, live.live.slide) : null}
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
                    <SlidePreview lines={slide.lines} image={thumbFor(viewed.id, i)} size="small" />
                    <span class="caption">
                      {#if isLive(viewed.id, i)}<StatusPill tone="live">LIVE</StatusPill>{/if}
                      <span>{slide.label ?? i + 1}</span>
                      {#if slide.steps > 0}<span class="faint steps">{slide.steps} builds</span>{/if}
                      <!-- The core labels video slides "Video"; the output draws nothing for them yet. -->
                      {#if slide.label === "Video"}<Placeholder ticket="T27">Not on screen yet</Placeholder>{/if}
                    </span>
                  </button>
                </li>
              {/each}
            </ul>
          {/if}
        </div>
      </section>

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
                image={thumbFor(upcoming.cursor.item, upcoming.cursor.slide)}
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
          <h2 class="title">Outputs</h2>
          <ul class="plain outputs">
            {#each live.outputs as o (o.id)}
              <li>
                <Dot tone={o.enabled && live.overlay === "None" && live.live ? "live" : "neutral"} />
                <span class="name">{o.name}{#if !isCalibrated(o)}<span class="warn"> · not calibrated</span>{/if}</span>
                <span class="muted small">{!o.enabled ? "Off" : live.overlay === "None" && !live.live ? "Nothing on air" : outputState[live.overlay]}</span>
              </li>
            {:else}
              <li class="faint small">No outputs. Add one in Outputs.</li>
            {/each}
          </ul>
        </section>
      </aside>
    </div>
  </div>
{/if}

<style>
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
  .bar {
    flex-shrink: 0;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem 1rem;
    padding: 0.6rem 1.25rem;
    border-bottom: 1px solid var(--line);
  }
  .service {
    font-size: 0.9rem;
  }
  .service strong {
    font-weight: 500;
  }
  .keys {
    margin-left: auto;
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.75rem;
  }
  .grid {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: 250px minmax(0, 1fr) 320px;
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
