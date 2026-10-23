<!-- Presenter view for a deck on air, as PowerPoint's: the slide as the
     audience sees it now (at its build step), what the next click shows
     (the next build or the next slide), the speaker notes, where the deck
     stands and how long it has been up. Next and Previous step through
     builds, then slides. -->
<script lang="ts">
  import type { Cursor, LoadedItemView, SafeArea } from "../../lib/api";
  import type { NextClick } from "../../lib/cursor";
  import { stepCounter, formatElapsed, slideCounter } from "../../lib/presenter";
  import Button from "../../lib/kit/Button.svelte";
  import Dot from "../../lib/kit/Dot.svelte";
  import SlidePreview from "../../lib/kit/SlidePreview.svelte";

  let {
    item,
    cursor,
    upcoming,
    current,
    nextImage,
    overlay,
    safeArea,
    showGuide,
    plannerNotes = "",
    onprevious,
    onnext,
    onblank,
  }: {
    /** The deck item on air. */
    item: LoadedItemView;
    cursor: Cursor;
    upcoming: NextClick | null;
    /** Pictures of the live slide at its step and of what the next click shows. */
    current?: string;
    nextImage?: string;
    overlay: "None" | "Black" | "Logo";
    safeArea?: SafeArea | null;
    showGuide: boolean;
    /** The planner's notes on the item, under the speaker notes. */
    plannerNotes?: string;
    onprevious: () => void;
    onnext: () => void;
    onblank: () => void;
  } = $props();

  let slide = $derived(item.slides[cursor.slide]);

  // A click asks for a new picture; until it arrives the last one stays,
  // so the large slide never flashes black between builds.
  let shown = $state<string | null>(null);
  $effect(() => {
    if (current) shown = current;
  });

  // The clock starts when the deck goes live (this view is made again for
  // each deck) and can be paused and reset.
  let started = $state(Date.now());
  let pausedAt = $state<number | null>(null);
  let now = $state(Date.now());
  $effect(() => {
    const timer = setInterval(() => (now = Date.now()), 250);
    return () => clearInterval(timer);
  });
  let elapsed = $derived((pausedAt ?? now) - started);

  function pause() {
    if (pausedAt === null) {
      pausedAt = Date.now();
    } else {
      started += Date.now() - pausedAt;
      pausedAt = null;
    }
  }

  function reset() {
    started = Date.now();
    now = started;
    if (pausedAt !== null) pausedAt = started;
  }

  const sizes = [0.95, 1.1, 1.3, 1.55, 1.85];
  let notesSize = $state(1);

  // Speaker notes can be hidden (N), for a speaker who knows the talk or a
  // small screen. The choice is remembered on this device; storage can be
  // missing or full, so reads and writes are wrapped.
  const NOTES_KEY = "lampstand.notes-shown";
  let notesShown = $state(true);
  try {
    notesShown = localStorage.getItem(NOTES_KEY) !== "false";
  } catch {
    // Shown, as by default.
  }
  function toggleNotes() {
    notesShown = !notesShown;
    try {
      localStorage.setItem(NOTES_KEY, String(notesShown));
    } catch {
      // Not remembered this time.
    }
  }
  function onkeydown(e: KeyboardEvent) {
    const target = e.target as HTMLElement | null;
    if (target && (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable)) return;
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    if (e.key === "n" || e.key === "N") {
      e.preventDefault();
      toggleNotes();
    }
  }

  // What the next click shows. A step stays on the same slide, so the label
  // names the slide plainly and puts the step in a quieter note beside it.
  let nextLabel = $derived(
    !upcoming
      ? "End of service"
      : upcoming.build
        ? `Next: Slide ${upcoming.cursor.slide + 1}`
        : upcoming.cursor.item === item.id
          ? `Next: ${slideCounter(upcoming.cursor.slide, item.slides.length).toLowerCase()}`
          : `Next: ${upcoming.title}`,
  );
  let nextStep = $derived(upcoming?.build ? `step ${upcoming.cursor.step} of ${upcoming.slide.steps}` : null);
</script>

<svelte:window {onkeydown} />

<div class="presenter">
  <div class="head">
    <span class="state"><Dot tone={overlay === "None" ? "live" : "neutral"} size={8} />{overlay === "None" ? "LIVE" : overlay === "Black" ? "BLANK" : "LOGO"}</span>
    <strong class="title">{item.title}</strong>
    <span class="counter">{slideCounter(cursor.slide, item.slides.length)}</span>
    {#if slide && stepCounter(cursor.step, slide.steps)}
      <span class="build">{stepCounter(cursor.step, slide.steps)}</span>
    {/if}
    <span class="clock" class:paused={pausedAt !== null} aria-label="Time on this deck" role="timer">{formatElapsed(elapsed)}</span>
    <Button size="small" variant="ghost" onclick={pause} title={pausedAt === null ? "Pause the timer" : "Resume the timer"}>
      {pausedAt === null ? "Pause" : "Resume"}
    </Button>
    <Button size="small" variant="ghost" onclick={reset} title="Start the timer again from zero">Reset</Button>
  </div>

  <div class="main">
    <section class="now" aria-label="On air now">
      <div class="screen" class:on={overlay === "None"}>
        <SlidePreview image={shown} {overlay} {safeArea} {showGuide} size="large" empty="Drawing the slide…" />
      </div>
      <div class="transport">
        <Button icon="prev" onclick={onprevious} shortcut="←">Previous</Button>
        <Button variant="primary" onclick={onnext} shortcut="Space">Next</Button>
        <Button variant="danger" onclick={onblank} pressed={overlay === "Black"} shortcut=".">Blank</Button>
      </div>
    </section>

    <aside class="aside">
      <section aria-label="Next">
        <h2 class="title label">
          <span class="where">{nextLabel}</span>
          {#if nextStep}<span class="step">{nextStep}</span>{/if}
        </h2>
        <div class="next">
          {#if upcoming}
            <SlidePreview lines={upcoming.slide.lines} footer={upcoming.slide.footer} image={nextImage} {safeArea} empty="" />
          {:else}
            <SlidePreview empty="End of service" />
          {/if}
        </div>
      </section>

      <section class="notes-pane" aria-label="Speaker notes">
        <div class="notes-head">
          <h2 class="title label">Speaker notes</h2>
          {#if notesShown}
            <Button size="small" variant="ghost" label="Smaller notes" disabled={notesSize === 0} onclick={() => (notesSize = Math.max(0, notesSize - 1))}>A−</Button>
            <Button size="small" variant="ghost" label="Larger notes" disabled={notesSize === sizes.length - 1} onclick={() => (notesSize = Math.min(sizes.length - 1, notesSize + 1))}>A+</Button>
          {/if}
          <Button size="small" variant="ghost" shortcut="N" onclick={toggleNotes} title={notesShown ? "Hide the speaker notes" : "Show the speaker notes"}>
            {notesShown ? "Hide" : "Show"}
          </Button>
        </div>
        {#if notesShown}
          {#if slide?.notes}
            <p class="notes" style="font-size: {sizes[notesSize]}rem">{slide.notes}</p>
          {:else}
            <p class="faint small">No speaker notes on this slide.</p>
          {/if}
        {/if}
      </section>

      {#if plannerNotes}
        <section aria-label="Planner notes">
          <h2 class="title label">Planner notes</h2>
          <p class="planner">{plannerNotes}</p>
        </section>
      {/if}
    </aside>
  </div>
</div>

<style>
  .presenter {
    container-type: inline-size;
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.4rem 0.75rem;
    font-size: 0.8rem;
  }
  .state {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    font-weight: 700;
    letter-spacing: 0.04em;
  }
  .title {
    font-weight: 500;
  }
  .counter {
    color: var(--text);
    font-variant-numeric: tabular-nums;
  }
  .build {
    color: var(--gold);
    font-variant-numeric: tabular-nums;
  }
  .clock {
    margin-left: auto;
    font-size: 1.1rem;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    letter-spacing: 0.02em;
  }
  .clock.paused {
    color: var(--faint);
  }
  .main {
    display: grid;
    grid-template-columns: minmax(0, 1.7fr) minmax(220px, 1fr);
    gap: 1.25rem;
    align-items: start;
  }
  .screen {
    border-radius: var(--radius-lg);
    overflow: hidden;
    box-shadow: 0 0 0 2px var(--line);
  }
  .screen.on {
    box-shadow: 0 0 0 2px var(--live);
  }
  .transport {
    display: flex;
    gap: 0.5rem;
    margin-top: 0.9rem;
  }
  .aside {
    display: flex;
    flex-direction: column;
    gap: 1.25rem;
    min-width: 0;
  }
  .label {
    font-size: 0.75rem;
    color: var(--muted);
  }
  /* The slide is the headline; the step is a quiet note beside it. */
  .where {
    color: var(--text);
    font-size: 0.95rem;
    font-weight: 600;
  }
  .step {
    margin-left: 0.5rem;
    color: var(--muted);
    font-size: 0.7rem;
    font-weight: 400;
  }
  .next {
    margin-top: 0.5rem;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    overflow: hidden;
  }
  .notes-head {
    display: flex;
    align-items: center;
    gap: 0.25rem;
  }
  .notes-head h2 {
    margin-right: auto;
  }
  .notes {
    margin-top: 0.5rem;
    line-height: 1.55;
    white-space: pre-wrap;
    max-height: 40vh;
    overflow-y: auto;
  }
  .planner {
    margin-top: 0.5rem;
    font-size: 0.85rem;
    line-height: 1.5;
    white-space: pre-wrap;
    color: var(--muted);
  }
  .small {
    font-size: 0.8rem;
    margin-top: 0.5rem;
  }
  @container (max-width: 640px) {
    .main {
      grid-template-columns: minmax(0, 1fr);
    }
  }
</style>
