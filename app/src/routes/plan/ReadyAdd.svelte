<!-- Add to Ready (Plan): a deck or media file from the Library,
     kept one tap away in the service's Ready list without going in the
     order. Type to filter; Enter adds the highlighted one; Esc closes.
     Items already in the order are pinned from the order instead. In
     Present, Quick add does this for decks and media. A deck's "One slide"
     shows its slides, to keep just one ready. -->
<script lang="ts">
  import { onMount } from "svelte";
  import { request, type Deck, type ItemKind, type MediaFile, type ReadyEntry } from "../../lib/api";
  import DeckSlidePicker from "../../lib/kit/DeckSlidePicker.svelte";
  import { report } from "../../lib/state.svelte";
  import Icon from "../../lib/kit/Icon.svelte";

  let {
    service,
    ready = {},
    onclose,
    onadded,
  }: {
    service: string;
    /** What the service keeps ready now, to mark slides already kept. */
    ready?: Record<string, ReadyEntry | undefined>;
    onclose: () => void;
    /** It went in: its title. */
    onadded: (title: string) => void;
  } = $props();

  interface Row {
    key: string;
    kind: "deck" | "media";
    title: string;
    item: ItemKind;
  }

  let query = $state("");
  let highlight = $state(0);
  let busy = $state(false);
  let loading = $state(true);
  let input: HTMLInputElement | undefined = $state();
  let all: Row[] = $state([]);
  /** A deck whose slides are shown, to keep one of them ready. */
  let slidesOf: { deck: string; title: string } | null = $state(null);
  let kept = $derived(
    new Set(
      Object.values(ready).flatMap((e) => (e && e.source !== "Order" && "Slide" in e.source && e.source.Slide.deck === slidesOf?.deck ? [e.source.Slide.slide] : [])),
    ),
  );
  async function keepSlide(slide: number) {
    if (!slidesOf || busy) return;
    busy = true;
    try {
      await request({ area: "Service", request: { AddReadySlide: { service, deck: slidesOf.deck, slide } } }, "Service");
      onadded(`${slidesOf.title} · slide ${slide + 1}`);
    } catch (err) {
      report(err);
      busy = false;
    }
  }

  onMount(() => {
    input?.focus();
    Promise.all([request({ area: "Library", request: "ListDecks" }, "Decks"), request({ area: "Library", request: "ListMedia" }, "MediaFiles")])
      .then(([decks, media]: [Deck[], MediaFile[]]) => {
        all = [
          ...decks.map<Row>((d) => ({ key: `deck:${d.id}`, kind: "deck", title: d.title, item: { Deck: { deck: d.id } } })),
          ...media.map<Row>((m) => ({ key: `media:${m.blob}`, kind: "media", title: m.name, item: { Media: { blob: m.blob, name: m.name, looped: false } } })),
        ].sort((a, b) => a.title.localeCompare(b.title));
      })
      .catch(report)
      .finally(() => (loading = false));
    const previous = document.activeElement as HTMLElement | null;
    return () => previous?.focus?.({ preventScroll: true });
  });

  let rows = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return q ? all.filter((r) => r.title.toLowerCase().includes(q)) : all;
  });
  $effect(() => {
    void query;
    highlight = 0;
  });

  async function add(row: Row | undefined) {
    if (!row || busy) return;
    busy = true;
    try {
      await request({ area: "Service", request: { AddReady: { service, kind: row.item } } }, "Service");
      onadded(row.title);
    } catch (err) {
      report(err);
      busy = false;
    }
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && slidesOf) {
      e.preventDefault();
      slidesOf = null;
    } else if (slidesOf) {
      return;
    } else if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      highlight = Math.min(highlight + 1, rows.length - 1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      highlight = Math.max(highlight - 1, 0);
    } else if (e.key === "Enter") {
      e.preventDefault();
      add(rows[highlight]);
    }
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="backdrop" onclick={onclose}>
  <div class="palette" role="dialog" aria-modal="true" aria-label="Add to Ready" tabindex="-1" onclick={(e) => e.stopPropagation()}>
    <div class="search">
      <Icon name="search" size={16} />
      <input
        bind:this={input}
        bind:value={query}
        {onkeydown}
        type="text"
        placeholder="A deck or a picture or video from the Library"
        autocomplete="off"
        spellcheck="false"
        aria-label="Deck or media to keep ready"
        aria-controls="ready-add-results"
      />
    </div>

    {#if slidesOf}
      <div class="slides-head">
        <button type="button" class="back" onclick={() => (slidesOf = null)}><Icon name="prev" size={14} />Back</button>
        <span>One slide of <strong>{slidesOf.title}</strong> to keep ready; its builds play by themselves</span>
      </div>
      <DeckSlidePicker deck={slidesOf.deck} title={slidesOf.title} {kept} onpick={keepSlide} />
    {:else}
    <ul id="ready-add-results" class="plain results" role="listbox" aria-label="Library">
      {#each rows as row, i (row.key)}
        <li role="option" aria-selected={i === highlight} class="result">
          <button type="button" class="row" class:active={i === highlight} class:has-more={row.kind === "deck"} disabled={busy} onmouseenter={() => (highlight = i)} onclick={() => add(row)}>
            <Icon name={row.kind} size={16} />
            <span class="title">{row.title}</span>
            <span class="sub">{row.kind === "deck" ? "Deck" : "Media"}</span>
          </button>
          {#if row.kind === "deck" && "Deck" in row.item}
            {@const deck = row.item.Deck.deck}
            <button type="button" class="one-slide" onclick={() => (slidesOf = { deck, title: row.title })} title="Keep just one slide of {row.title} ready">One slide <Icon name="next" size={12} /></button>
          {/if}
        </li>
      {:else}
        <li class="hint">
          {#if loading}Looking in the Library…{:else if query.trim()}Nothing in the Library by that name.{:else}No decks or media in the Library yet. Add them in Plan.{/if}
        </li>
      {/each}
    </ul>
    {/if}

    <p class="keys faint">
      For an item already in the order, use <Icon name="star" size={12} /> Keep ready on it · <kbd>Enter</kbd> add · <kbd>Esc</kbd> close
    </p>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: flex;
    justify-content: center;
    align-items: flex-start;
    padding-top: 12vh;
    background: rgb(0 0 0 / 0.5);
  }
  .palette {
    width: min(36rem, calc(100% - 2rem));
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    box-shadow: 0 1rem 3rem rgb(0 0 0 / 0.5);
    overflow: hidden;
  }
  .search {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    padding: 0.75rem 1rem;
    border-bottom: 1px solid var(--line);
    color: var(--muted);
  }
  .search input {
    flex: 1;
    min-width: 0;
    font: inherit;
    font-size: 1rem;
    color: var(--text);
    background: transparent;
    border: 0;
    outline: 0;
  }
  .results {
    max-height: 50vh;
    overflow-y: auto;
  }
  .row {
    display: flex;
    align-items: baseline;
    gap: 0.6rem;
    width: 100%;
    padding: 0.55rem 1rem;
    font: inherit;
    text-align: left;
    color: var(--text);
    background: transparent;
    border: 0;
    cursor: pointer;
  }
  .result {
    position: relative;
  }
  .row.has-more {
    padding-right: 7rem;
  }
  .one-slide {
    position: absolute;
    top: 50%;
    right: 0.75rem;
    transform: translateY(-50%);
    display: inline-flex;
    align-items: center;
    gap: 0.2rem;
    padding: 0.15rem 0.5rem;
    font: inherit;
    font-size: 0.75rem;
    color: var(--muted);
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: 999px;
    cursor: pointer;
  }
  .one-slide:hover {
    color: var(--text);
    border-color: var(--gold-hover);
  }
  .slides-head {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding: 0.6rem 1rem 0;
    font-size: 0.8125rem;
    color: var(--muted);
  }
  .back {
    display: inline-flex;
    align-items: center;
    gap: 0.2rem;
    padding: 0.15rem 0.5rem;
    font: inherit;
    color: var(--text);
    background: none;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    cursor: pointer;
  }
  .row.active {
    background: var(--hover, rgb(255 255 255 / 0.07));
  }
  .row:disabled {
    cursor: default;
    opacity: 0.7;
  }
  .row :global(svg) {
    align-self: center;
    flex-shrink: 0;
    color: var(--muted);
  }
  .title {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 500;
  }
  .sub {
    margin-left: auto;
    font-size: 0.8125rem;
    color: var(--muted);
  }
  .hint {
    padding: 0.9rem 1rem;
    font-size: 0.875rem;
    color: var(--muted);
  }
  .keys {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.25rem;
    padding: 0.5rem 1rem;
    border-top: 1px solid var(--line);
    font-size: 0.75rem;
  }
  kbd {
    font: inherit;
    padding: 0 0.25rem;
    border: 1px solid var(--line);
    border-radius: 3px;
  }
</style>
