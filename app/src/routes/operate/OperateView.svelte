<!-- Bare operator panel: the loaded service's slides, the live slide's
     text, Next / Previous / Black / Clear with keyboard shortcuts.
     T47 replaces this with the full operator view. -->
<script lang="ts">
  import { send, type Command, type Cursor } from "../../lib/api";
  import { live, report } from "../../lib/state.svelte";
  import { sectionCursor } from "../../lib/sections";
  import Button from "../../lib/kit/Button.svelte";
  import Panel from "../../lib/kit/Panel.svelte";

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
</script>

<svelte:window {onkeydown} />

{#if !live.loaded}
  <p class="muted">No service is open. Open one from Plan.</p>
{:else}
  <div class="controls">
    <Button onclick={() => command("Previous")} shortcut="←">Previous</Button>
    <Button variant="primary" onclick={() => command("Next")} shortcut="Space">Next</Button>
    <Button variant="danger" onclick={() => command("ToggleBlack")} pressed={live.overlay === "Black"} shortcut=".">
      Black
    </Button>
    <Button onclick={() => command("ClearAll")}>Clear</Button>
  </div>

  <p class="muted keys">Keys: Space or → next, ← previous, . black, 1–9 verse, C chorus, B bridge, P pre-chorus, T tag, I intro, E ending</p>

  <div class="grid">
    <Panel title={live.loaded.title}>
      {#each live.loaded.items as item (item.id)}
        <h3>{item.title}</h3>
        {#if item.slides.length === 0}
          <p class="muted">Nothing to show.</p>
        {/if}
        <div class="slides">
          {#each item.slides as slide, i}
            <button
              class="slide"
              class:live={isLive(item.id, i)}
              onclick={() => show({ item: item.id, slide: i, step: 0 })}
              aria-current={isLive(item.id, i) ? "true" : undefined}
            >
              <span class="label">{slide.label ?? i + 1}</span>
              {#each slide.lines as line}<span>{line}</span>{/each}
            </button>
          {/each}
        </div>
      {/each}
    </Panel>

    <Panel title="Live">
      <div class="onair" class:black={live.overlay === "Black"} aria-live="polite">
        {#if live.overlay === "Black"}
          <span class="muted">Black</span>
        {:else if live.liveSlide}
          {#each live.liveSlide.lines as line}<p>{line}</p>{/each}
        {:else}
          <span class="muted">Nothing on air</span>
        {/if}
      </div>
    </Panel>
  </div>
{/if}

<style>
  .controls {
    display: flex;
    gap: 0.5rem;
    margin-bottom: 1rem;
  }
  .keys {
    margin: -0.4rem 0 0.8rem;
    font-size: 0.85rem;
  }
  .grid {
    display: grid;
    grid-template-columns: 2fr 1fr;
    gap: 1rem;
    align-items: start;
  }
  h3 {
    font-size: 1rem;
    margin: 0.8rem 0 0.4rem;
  }
  .slides {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(190px, 1fr));
    gap: 0.5rem;
  }
  .slide {
    font: inherit;
    font-size: 0.82rem;
    text-align: left;
    color: var(--text);
    background: var(--raised);
    border: 2px solid var(--line);
    border-radius: 6px;
    padding: 0.4rem 0.5rem;
    display: flex;
    flex-direction: column;
    cursor: pointer;
  }
  .slide:focus-visible {
    outline: 2px solid var(--accent);
  }
  .slide.live {
    border-color: var(--live);
  }
  .label {
    font-weight: 600;
    color: var(--muted);
  }
  .onair {
    background: #000;
    border-radius: 6px;
    aspect-ratio: 16 / 9;
    display: flex;
    flex-direction: column;
    justify-content: center;
    align-items: center;
    text-align: center;
    padding: 0.5rem;
    font-size: 0.9rem;
  }
  .onair p {
    margin: 0.1rem 0;
  }
</style>
