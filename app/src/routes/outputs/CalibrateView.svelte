<!-- Calibrating one screen: set how far in from each edge text must stay.
     The margins are the output's safe area and save through
     OutputRequest::Put. Putting the numbered grid on the screen itself,
     so someone in the room can read it, arrives with T26; until then the
     grid shows here only.
     A screen of another shape than the projector's picture (a 4:5 screen
     under a 16:9 projector) starts from a shape preset; backgrounds can
     then stay on the screen too (BackgroundMode::Fit). -->
<script lang="ts">
  import { untrack } from "svelte";
  import type { BackgroundMode, OutputConfig, SafeArea } from "../../lib/api";
  import { marginsForScreen, screenShapes, targetSize, type ScreenShape } from "../../lib/screenShape";
  import Button from "../../lib/kit/Button.svelte";
  import Placeholder from "../../lib/kit/Placeholder.svelte";
  import Segmented from "../../lib/kit/Segmented.svelte";
  import SlidePreview from "../../lib/kit/SlidePreview.svelte";

  let {
    output,
    onsave,
    oncancel,
  }: {
    output: OutputConfig;
    onsave: (safeArea: SafeArea, background: BackgroundMode) => void;
    oncancel: () => void;
  } = $props();

  // Edges in percent while editing, starting from the saved margins; the
  // API holds fractions.
  const start = untrack(() => output.safe_area);
  const size = untrack(() => targetSize(output.target));
  let background: BackgroundMode = $state(untrack(() => output.background));
  const backgrounds = [
    { id: "Bleed", label: "Fill the projector" },
    { id: "Fit", label: "Only on the screen" },
  ] as const;
  let shape: ScreenShape | null = $state(null);

  function useShape(id: ScreenShape) {
    shape = id;
    const ratio = screenShapes.find((s) => s.id === id)?.ratio ?? null;
    const m = marginsForScreen(size.width, size.height, ratio);
    edges = { top: percent(m.top), right: percent(m.right), bottom: percent(m.bottom), left: percent(m.left) };
    // Light outside a smaller screen lands on the wall.
    if (ratio !== null) background = "Fit";
  }
  const percent = (f: number) => Math.round(f * 1000) / 10;
  let edges = $state({
    top: percent(start.top),
    right: percent(start.right),
    bottom: percent(start.bottom),
    left: percent(start.left),
  });
  const sides = ["top", "right", "bottom", "left"] as const;
  /** Grid numbers, per cent in from each edge: sides go further, for narrow screens. */
  const ticks = [0, 5, 10, 15, 20];
  const sideTicks = [0, 5, 10, 15, 20, 25, 30, 35, 40];

  let view: "grid" | "sample" = $state("grid");
  const views = [
    { id: "grid", label: "Numbered grid" },
    { id: "sample", label: "Sample lyrics" },
  ] as const;

  // Up to 45% a side, so a narrow screen fits and some picture is left.
  const clamp = (n: number) => (Number.isFinite(n) ? Math.min(45, Math.max(0, n)) : 0);
  let fraction = $derived<SafeArea>({
    top: clamp(edges.top) / 100,
    right: clamp(edges.right) / 100,
    bottom: clamp(edges.bottom) / 100,
    left: clamp(edges.left) / 100,
  });
</script>

<div class="calibrate">
  <section class="stage" aria-labelledby="calibrate-title">
    <div class="head">
      <Button variant="ghost" icon="prev" onclick={oncancel}>All outputs</Button>
      <h1 id="calibrate-title">Calibrate {output.name}</h1>
      <Placeholder ticket="T26">Grid on the screen itself</Placeholder>
    </div>

    <div class="frame">
      {#if view === "grid"}
        <div class="grid-screen" aria-label="Numbered grid with the safe area" role="img">
          {#each ticks as t}
            <span class="tick top" style:left="50%" style:top="{t}%">{t}</span>
            <span class="tick bottom" style:left="50%" style:bottom="{t}%">{t}</span>
          {/each}
          {#each sideTicks as t}
            <span class="tick left" style:top="50%" style:left="{t}%">{t}</span>
            <span class="tick right" style:top="50%" style:right="{t}%">{t}</span>
          {/each}
          <div
            class="guide"
            style:top="{fraction.top * 100}%"
            style:right="{fraction.right * 100}%"
            style:bottom="{fraction.bottom * 100}%"
            style:left="{fraction.left * 100}%"
          ></div>
        </div>
      {:else}
        <SlidePreview
          lines={["Amazing grace! How sweet the sound", "That saved a wretch like me!", "I once was lost, but now am found;", "Was blind, but now I see."]}
          safeArea={fraction}
          showGuide
          size="large"
        />
      {/if}
    </div>

    <div class="views"><Segmented options={views} value={view} onchange={(v) => (view = v)} label="What the preview shows" /></div>
  </section>

  <aside class="pane-side numbers" aria-label="Edge margins">
    <h2>Screen shape</h2>
    <p class="muted">If the screen is narrower or wider than the projector's picture, start from its shape.</p>
    <div class="shape">
      <Segmented options={screenShapes} value={shape ?? ("" as ScreenShape)} onchange={useShape} label="Screen shape" stretch />
    </div>

    <h2 class="next">Read the numbers</h2>
    <p class="muted">
      Ask someone in the room for the smallest number they can fully see on each edge, and type it in. The safe area
      moves as you type. Each number is a percentage of the screen's height (top, bottom) or width (left, right).
    </p>

    <div class="edges">
      {#each sides as side}
        <label class="edge {side}">
          <span>{side[0].toUpperCase() + side.slice(1)}</span>
          <input type="number" min="0" max="45" step="0.5" bind:value={edges[side]} oninput={() => (shape = null)} />
        </label>
      {/each}
      <div class="mini" aria-hidden="true">
        <div
          class="mini-guide"
          style:top="{fraction.top * 100}%"
          style:right="{fraction.right * 100}%"
          style:bottom="{fraction.bottom * 100}%"
          style:left="{fraction.left * 100}%"
        ></div>
      </div>
    </div>

    <p class="faint summary num">
      Margins · top {clamp(edges.top)}% · right {clamp(edges.right)}% · bottom {clamp(edges.bottom)}% · left {clamp(edges.left)}%
    </p>

    <h2 class="next">Backgrounds</h2>
    <p class="muted">Text always stays inside the safe area. Backgrounds and video can too, so nothing lands on the wall.</p>
    <div class="shape">
      <Segmented options={backgrounds} value={background} onchange={(b) => (background = b)} label="Backgrounds" stretch />
    </div>

    <div class="save">
      <Button variant="primary" onclick={() => onsave(fraction, background)}>Save safe area</Button>
      <Button variant="ghost" onclick={oncancel}>Cancel</Button>
    </div>
  </aside>
</div>

<style>
  .calibrate {
    height: 100%;
    display: grid;
    grid-template-columns: minmax(0, 1fr) 340px;
  }
  .stage {
    display: flex;
    flex-direction: column;
    gap: 1rem;
    padding: 1.5rem 2rem;
    min-height: 0;
    overflow-y: auto;
  }
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.75rem;
  }
  h1 {
    font-size: 1.15rem;
    font-weight: 600;
  }
  .frame {
    width: 100%;
    max-width: calc((100vh - 18rem) * 16 / 9);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    overflow: hidden;
  }
  .grid-screen {
    position: relative;
    aspect-ratio: 16 / 9;
    background:
      repeating-linear-gradient(to right, rgba(255, 255, 255, 0.07) 0 1px, transparent 1px 5%),
      repeating-linear-gradient(to bottom, rgba(255, 255, 255, 0.07) 0 1px, transparent 1px 5%),
      #000;
  }
  .tick {
    position: absolute;
    font-size: 0.75rem;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    color: #fff;
    line-height: 1;
  }
  .tick.top {
    transform: translate(-50%, 0);
  }
  .tick.bottom {
    transform: translate(-50%, 0);
  }
  .tick.left {
    transform: translate(0, -50%);
  }
  .tick.right {
    transform: translate(0, -50%);
  }
  .guide {
    position: absolute;
    border: 1px dashed var(--gold);
    background: var(--gold-soft);
  }
  .views {
    align-self: flex-start;
  }
  .numbers {
    border-left: 1px solid var(--line);
    padding: 1.5rem;
    display: flex;
    flex-direction: column;
    overflow-y: auto;
  }
  h2 {
    font-size: 1rem;
    font-weight: 600;
  }
  .numbers > p.muted {
    margin-top: 0.4rem;
    font-size: 0.85rem;
    line-height: 1.6;
  }
  .shape {
    margin-top: 0.75rem;
  }
  h2.next {
    margin-top: 1.75rem;
  }
  .edges {
    margin-top: 1.5rem;
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    grid-template-rows: repeat(3, auto);
    align-items: center;
    justify-items: center;
    gap: 1rem 0.5rem;
  }
  .edge {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.25rem;
    font-size: 0.75rem;
    color: var(--muted);
  }
  .edge input {
    width: 4rem;
    text-align: center;
    font-variant-numeric: tabular-nums;
  }
  .edge.top {
    grid-area: 1 / 2;
  }
  .edge.left {
    grid-area: 2 / 1;
  }
  .edge.right {
    grid-area: 2 / 3;
  }
  .edge.bottom {
    grid-area: 3 / 2;
  }
  .mini {
    grid-area: 2 / 2;
    position: relative;
    width: 86px;
    aspect-ratio: 16 / 9;
    border: 1px solid var(--line-strong);
    border-radius: 3px;
  }
  .mini-guide {
    position: absolute;
    border: 1px dashed var(--gold);
  }
  .summary {
    margin-top: 1.5rem;
    text-align: center;
    font-size: 0.75rem;
  }
  .save {
    margin-top: auto;
    padding-top: 2rem;
    display: flex;
    gap: 0.5rem;
  }
</style>
