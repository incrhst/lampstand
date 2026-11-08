<!-- A deck's slides as pictures, to pick one: for keeping one slide ready
     (Quick add in Present, Add to Ready in Plan). Slides count from 0 as the
     deck is shown, hidden slides left out (`LibraryRequest::DeckSlidePicture`).
     The pictures come one at a time, the first telling how many there are.
     The keyboard works it from the dialog's search box, as the results list
     is: the dialog passes its keys to `key`, and arrows move the highlight
     and Enter picks the highlighted slide (Shift+Enter shows it now too). -->
<script lang="ts">
  import { request } from "../api";

  let {
    deck,
    title,
    kept = new Set<number>(),
    onpick,
  }: {
    deck: string;
    title: string;
    /** Slides of this deck already kept ready. */
    kept?: Set<number>;
    /** The slide picked, and whether Shift was held (to show it now as well). */
    onpick: (slide: number, shift: boolean) => void;
  } = $props();

  const WIDTH = 240;
  let count: number | null = $state(null);
  let pictures: Record<number, string> = $state({});
  let failed = $state(false);
  /** The slide the keyboard is on. */
  let highlight = $state(0);
  let list: HTMLUListElement | undefined = $state();
  /** Gives each picker's slides ids of their own, for aria-activedescendant. */
  const uid = `deck-slides-${Math.random().toString(36).slice(2, 8)}`;

  /** The id of the highlighted slide's option, for the search box's aria-activedescendant. */
  export function activeId(): string | undefined {
    return count ? `${uid}-${highlight}` : undefined;
  }

  /** How many slides sit in one row of the grid. */
  function columns(): number {
    if (!list) return 1;
    return Math.max(1, getComputedStyle(list).gridTemplateColumns.split(" ").filter(Boolean).length);
  }

  /**
   * Handles a key from the dialog while the slides are showing. Arrows move
   * (up and down by a row), Home and End go to the first and last slide,
   * Enter picks. Returns whether the key was used, so the dialog can stop it.
   */
  export function key(e: KeyboardEvent): boolean {
    if (!count) return e.key === "Enter";
    const step: Record<string, number> = { ArrowRight: 1, ArrowLeft: -1, ArrowDown: columns(), ArrowUp: -columns() };
    if (e.key in step) {
      const to = highlight + step[e.key];
      if (to >= 0 && to < count) highlight = to;
    } else if (e.key === "Home") {
      highlight = 0;
    } else if (e.key === "End") {
      highlight = count - 1;
    } else if (e.key === "Enter") {
      onpick(highlight, e.shiftKey);
    } else {
      return false;
    }
    return true;
  }

  $effect(() => {
    void highlight;
    list?.querySelector(".slide.active")?.scrollIntoView({ block: "nearest" });
  });

  $effect(() => {
    const id = deck;
    let current = true;
    count = null;
    highlight = 0;
    pictures = {};
    failed = false;
    (async () => {
      try {
        const first = await request({ area: "Library", request: { DeckSlidePicture: { deck: id, slide: 0, width: WIDTH } } }, "DeckSlide");
        if (!current) return;
        pictures[0] = first.thumbnail;
        count = first.slides;
        for (let i = 1; i < first.slides && current; i++) {
          try {
            const next = await request({ area: "Library", request: { DeckSlidePicture: { deck: id, slide: i, width: WIDTH } } }, "DeckSlide");
            if (current) pictures[i] = next.thumbnail;
          } catch {
            // That slide shows its number only.
          }
        }
      } catch {
        if (current) failed = true;
      }
    })();
    return () => {
      current = false;
    };
  });
</script>

{#if failed}
  <p class="hint">The slides of {title} can't be shown on this computer yet.</p>
{:else if count === null}
  <p class="hint">Getting the slides of {title}…</p>
{:else}
  <ul bind:this={list} class="plain slides" role="listbox" aria-label="Slides of {title}">
    {#each Array.from({ length: count }, (_, i) => i) as i}
      <li role="option" id="{uid}-{i}" aria-selected={i === highlight}>
        <button type="button" class="slide" class:kept={kept.has(i)} class:active={i === highlight} onmouseenter={() => (highlight = i)} onclick={(e) => onpick(i, e.shiftKey)} title={kept.has(i) ? `Slide ${i + 1} is already kept ready` : `Keep slide ${i + 1} ready`}>
          {#if pictures[i]}<img src={pictures[i]} alt="" />{:else}<span class="blank"></span>{/if}
          <span class="num">{i + 1}{#if kept.has(i)} · ready{/if}</span>
        </button>
      </li>
    {/each}
  </ul>
{/if}

<style>
  .hint {
    padding: 0.9rem 1rem;
    font-size: 0.875rem;
    color: var(--muted);
  }
  .slides {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(8.5rem, 1fr));
    gap: 0.5rem;
    padding: 0.75rem 1rem;
    max-height: 50vh;
    overflow-y: auto;
  }
  .slide {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    width: 100%;
    padding: 0.2rem;
    font: inherit;
    font-size: 0.75rem;
    color: var(--text);
    background: none;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    cursor: pointer;
  }
  .slide:hover,
  .slide:focus-visible {
    border-color: var(--gold-hover);
  }
  .slide.kept {
    border-color: var(--gold);
  }
  /* The keyboard's slide: the same gold ring as the focus ring. */
  .slide.active {
    border-color: var(--gold);
    box-shadow: 0 0 0 2px var(--gold);
  }
  img,
  .blank {
    display: block;
    width: 100%;
    aspect-ratio: 16 / 9;
    object-fit: contain;
    background: #000;
    border-radius: 2px;
  }
  .num {
    color: var(--muted);
  }
</style>
