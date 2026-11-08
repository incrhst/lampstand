<!-- Quick add (Present): the Library search of the sidecard. Type a song
     title or a reference ("Rom 8:28") and put it in the service that is open
     for live, without going back to Plan: Enter adds it right after what is
     on air; Shift+Enter adds it and puts it on air now; Ctrl or Cmd+Enter
     adds it at the end. A deck or media file from the Library goes to Ready
     instead (lib/ready.ts), never the order: Enter keeps it ready, Shift+Enter
     keeps it ready and shows it now, as a side-trip. A deck's "One slide"
     (or the right arrow) shows its slides to keep just one ready, chosen with
     the arrow keys and Enter or a click; its builds
     then play by themselves. -->
<script lang="ts">
  import { onMount } from "svelte";
  import { request, type Deck, type ItemKind, type MediaFile, type SongHit, type Translation } from "../../lib/api";
  import { mightBeReference } from "../../lib/library";
  import { formatPassage, parseReferenceVersioned } from "../../lib/passage";
  import { chooseTranslation } from "../../lib/bibles";
  import { loadDefaultTranslation, saveDefaultTranslation } from "../../lib/bibleSetting";
  import { entryAction, insertionIndex, newItemId } from "../../lib/quickAdd";
  import { report } from "../../lib/state.svelte";
  import Icon from "../../lib/kit/Icon.svelte";
  import { modal } from "../../lib/modal";
  import DeckSlidePicker from "../../lib/kit/DeckSlidePicker.svelte";

  let {
    service,
    order,
    liveItem,
    readyIds = [],
    forReady = false,
    keptSlides = {},
    onclose,
    onadded,
    onreadied,
  }: {
    /** The service open for live. */
    service: string;
    /** Its items, in order, for finding where "next" is. */
    order: string[];
    /** The item on air, or null. */
    liveItem: string | null;
    /** What the service keeps ready now, for finding a new entry. */
    readyIds?: string[];
    /** Opened from the Ready card: the Library's decks and media show before anything is typed. */
    forReady?: boolean;
    /** Slides already kept ready one by one, by deck. */
    keptSlides?: Record<string, number[]>;
    onclose: () => void;
    /** An item went in: its ID (null if it could not be found), whether to put it on air, and its title. */
    onadded: (id: string | null, show: boolean, title: string) => void;
    /** A deck or media went to Ready: its ID (null if it could not be found), whether to show it now, and its title. */
    onreadied: (id: string | null, show: boolean, title: string) => void;
  } = $props();

  interface Row {
    key: string;
    kind: "song" | "scripture" | "deck" | "media";
    title: string;
    subtitle: string;
    /** What adding inserts; null while a lookup is still running or if it failed. */
    item: ItemKind | null;
  }

  let query = $state("");
  let highlight = $state(0);
  let busy = $state(false);
  let input: HTMLInputElement | undefined = $state();

  let songs: SongHit[] = $state([]);
  let translations: Translation[] = $state([]);
  let translation = $state("");
  let passage: Row | null = $state(null);
  /** The Library's decks and media, which go to Ready. */
  let files: Row[] = $state([]);
  /** A deck whose slides are shown, to keep one of them ready. */
  let picker: DeckSlidePicker | undefined = $state();
  let slidesOf: { deck: string; title: string } | null = $state(null);
  function openSlides(row: Row | undefined) {
    if (row?.item && "Deck" in row.item) slidesOf = { deck: row.item.Deck.deck, title: row.title };
  }
  async function keepSlide(slide: number, show: boolean) {
    if (!slidesOf || busy) return;
    busy = true;
    const title = `${slidesOf.title} · slide ${slide + 1}`;
    try {
      const updated = await request({ area: "Service", request: { AddReadySlide: { service, deck: slidesOf.deck, slide } } }, "Service");
      const id = newItemId(readyIds, Object.keys(updated.ready ?? {})) ?? Object.entries(updated.ready ?? {}).find(([, e]) => e.source !== "Order" && "Slide" in e.source && e.source.Slide.deck === slidesOf?.deck && e.source.Slide.slide === slide)?.[0] ?? null;
      onreadied(id, show, title);
    } catch (err) {
      report(err);
      busy = false;
    }
  }
  let trimmed = $derived(query.trim());

  const labelOf = (id: string) => {
    const t = translations.find((t) => t.id === id);
    return t?.abbreviation || t?.name || "";
  };
  let abbreviation = $derived(translations.find((t) => t.id === translation)?.abbreviation || translations.find((t) => t.id === translation)?.name || "");

  onMount(() => {
    input?.focus();
    Promise.all([request({ area: "Library", request: "ListDecks" }, "Decks"), request({ area: "Library", request: "ListMedia" }, "MediaFiles")])
      .then(([decks, media]: [Deck[], MediaFile[]]) => {
        files = [
          ...decks.map<Row>((d) => ({ key: `deck:${d.id}`, kind: "deck", title: d.title, subtitle: "Deck · to Ready", item: { Deck: { deck: d.id } } })),
          ...media.map<Row>((m) => ({ key: `media:${m.blob}`, kind: "media", title: m.name, subtitle: "Media · to Ready", item: { Media: { blob: m.blob, name: m.name, looped: false } } })),
        ].sort((a, b) => a.title.localeCompare(b.title));
      })
      .catch(report);
    Promise.all([request({ area: "Library", request: "ListTranslations" }, "Translations"), loadDefaultTranslation()])
      .then(([all, preferred]) => {
        translations = [...all].sort((a, b) => a.name.localeCompare(b.name));
        translation = chooseTranslation(translations, preferred, translation);
      })
      .catch(report);
  });

  // Each search is numbered so a slow answer to an old query never replaces
  // a newer one.
  let songSeq = 0;
  $effect(() => {
    const q = trimmed;
    const seq = ++songSeq;
    if (!q) {
      songs = [];
      return;
    }
    request({ area: "Library", request: { SearchSongs: { query: q, limit: 8 } } }, "Songs")
      .then((found) => {
        if (seq === songSeq) songs = found;
      })
      .catch(report);
  });

  let lookupSeq = 0;
  $effect(() => {
    const typed = mightBeReference(trimmed) ? parseReferenceVersioned(trimmed) : null;
    const t = translation;
    const seq = ++lookupSeq;
    if (!typed) {
      passage = null;
      return;
    }
    const title = formatPassage(typed.passage);
    if (!t) {
      passage = { key: "p", kind: "scripture", title, subtitle: "No Bible on this device yet. Import one in Plan.", item: null };
      return;
    }
    passage = { key: "p", kind: "scripture", title, subtitle: "Looking up…", item: null };
    request({ area: "Library", request: { LookupPassage: { reference: trimmed, translation: t } } }, "Passage")
      .then((found) => {
        if (seq !== lookupSeq) return;
        passage = {
          key: "p",
          kind: "scripture",
          title: found.display,
          subtitle: `${labelOf(found.translation)} · ${found.verses[0]?.[2] ?? ""}`,
          item: { Scripture: { passage: found.passage, translation: found.translation } },
        };
      })
      .catch((e) => {
        if (seq !== lookupSeq) return;
        passage = { key: "p", kind: "scripture", title, subtitle: e instanceof Error ? e.message : String(e), item: null };
      });
  });

  /** Decks and media whose name has what is typed; all of them, from the Ready card, before anything is typed. */
  let fileRows = $derived.by(() => {
    const q = trimmed.toLowerCase();
    if (!q) return forReady ? files : [];
    return files.filter((f) => f.title.toLowerCase().includes(q)).slice(0, 6);
  });
  let rows: Row[] = $derived([
    ...(forReady ? fileRows : []),
    ...(passage ? [passage] : []),
    ...songs.map<Row>((h) => ({
      key: `song:${h.song}`,
      kind: "song",
      title: h.title,
      subtitle: h.line ? `“${h.line}”` : "Song",
      item: { Song: { song: h.song, arrangement: null } },
    })),
    ...(forReady ? [] : fileRows),
  ]);

  $effect(() => {
    void trimmed;
    highlight = 0;
  });
  $effect(() => {
    if (highlight >= rows.length) highlight = Math.max(0, rows.length - 1);
  });

  async function add(row: Row | undefined, e: { shiftKey: boolean; metaKey: boolean; ctrlKey: boolean }) {
    if (!row?.item || busy) return;
    const { where, show } = entryAction(e);
    busy = true;
    if (row.kind === "deck" || row.kind === "media") {
      try {
        const updated = await request({ area: "Service", request: { AddReady: { service, kind: row.item } } }, "Service");
        onreadied(newItemId(readyIds, Object.keys(updated.ready ?? {})), e.shiftKey, row.title);
      } catch (err) {
        report(err);
        busy = false;
      }
      return;
    }
    try {
      const updated = await request(
        { area: "Service", request: { InsertItem: { service, index: insertionIndex(order, liveItem, where), kind: row.item } } },
        "Service",
      );
      onadded(newItemId(order, Object.keys(updated.items)), show, row.title);
    } catch (err) {
      report(err);
      busy = false;
    }
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && slidesOf) {
      e.preventDefault();
      slidesOf = null;
    } else if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    } else if (!slidesOf && e.key === "ArrowRight" && rows[highlight]?.kind === "deck" && input?.selectionStart === query.length) {
      e.preventDefault();
      openSlides(rows[highlight]);
    } else if (slidesOf) {
      // The arrows choose a slide and Enter keeps it ready; Tab stays put.
      if (picker?.key(e) || e.key === "Tab") e.preventDefault();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      highlight = Math.min(highlight + 1, rows.length - 1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      highlight = Math.max(highlight - 1, 0);
    } else if (e.key === "Enter") {
      e.preventDefault();
      add(rows[highlight], e);
    } else if (e.key === "Tab" && translations.length > 1) {
      // Switch Bible without leaving the keyboard.
      e.preventDefault();
      const at = translations.findIndex((t) => t.id === translation);
      translation = translations[(at + 1) % translations.length].id;
      // The last Bible chosen is the default for the next reference.
      saveDefaultTranslation(translation).catch(report);
    }
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="backdrop" onclick={onclose}>
  <div class="palette" role="dialog" aria-modal="true" aria-label="Quick add" tabindex="-1" use:modal onclick={(e) => e.stopPropagation()}>
    <div class="search">
      <Icon name="search" size={16} />
      <input
        bind:this={input}
        bind:value={query}
        {onkeydown}
        type="text"
        placeholder={forReady ? "A deck, picture or video to add to Ready" : "Song, reference like Rom 8:28, deck or media"}
        autocomplete="off"
        spellcheck="false"
        aria-label={forReady ? "Deck or media to add to Ready" : "Song, scripture, deck or media to add"}
        aria-controls="quick-add-results"
        aria-activedescendant={slidesOf ? picker?.activeId() : undefined}
      />
      {#if translations.length > 0 && passage}<span class="tag" title="Tab changes Bible">{abbreviation}</span>{/if}
    </div>

    {#if slidesOf}
      <div class="slides-head">
        <button type="button" class="back" onclick={() => (slidesOf = null)}><Icon name="prev" size={14} />Back</button>
        <span>One slide of <strong>{slidesOf.title}</strong> to add to Ready; its builds play by themselves</span>
      </div>
      <DeckSlidePicker bind:this={picker} deck={slidesOf.deck} title={slidesOf.title} kept={new Set(keptSlides[slidesOf.deck] ?? [])} onpick={keepSlide} />
    {:else}
    <ul id="quick-add-results" class="plain results" role="listbox" aria-label="Results">
      {#each rows as row, i (row.key)}
        <li role="option" aria-selected={i === highlight} class="result">
          <button type="button" class="row" class:active={i === highlight} class:has-more={row.kind === "deck"} disabled={!row.item} onmouseenter={() => (highlight = i)} onclick={(e) => add(row, e)}>
            <Icon name={row.kind} size={16} />
            <span class="title">{row.title}</span>
            <span class="sub">{row.subtitle}</span>
          </button>
          {#if row.kind === "deck"}
            <button type="button" class="one-slide" onmouseenter={() => (highlight = i)} onclick={() => openSlides(row)} title="Keep just one slide of {row.title} ready (→)">One slide <Icon name="next" size={12} /></button>
          {/if}
        </li>
      {:else}
        <li class="hint">
          {#if forReady && files.length === 0}No decks or media in the Library yet. Add them in Plan.{:else}{trimmed ? "Nothing found." : "Type a song title, a reference like John 3:16, or a deck or media name."}{/if}
        </li>
      {/each}
    </ul>
    {/if}

    <p class="keys faint">
      {#if slidesOf}
        Arrows choose a slide · <kbd>Enter</kbd> add it to Ready · <kbd>⇧ Enter</kbd> and show now · <kbd>Esc</kbd> back
      {:else if rows[highlight]?.kind === "deck"}
        <kbd>Enter</kbd> add the deck to Ready · <kbd>⇧ Enter</kbd> and show now · <kbd>→</kbd> one slide · <kbd>Esc</kbd> close
      {:else if rows[highlight]?.kind === "media"}
        <kbd>Enter</kbd> add to Ready · <kbd>⇧ Enter</kbd> add to Ready and show now · decks and media go to Ready, not the order · <kbd>Esc</kbd> close
      {:else}
        <kbd>Enter</kbd> add next · <kbd>⇧ Enter</kbd> add and show now · <kbd>⌃/⌘ Enter</kbd> add at end{#if translations.length > 1} · <kbd>Tab</kbd> Bible{/if} · <kbd>Esc</kbd> close
      {/if}
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
  .tag {
    font-size: 0.75rem;
    padding: 0.1rem 0.45rem;
    border: 1px solid var(--line);
    border-radius: var(--radius);
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
    font-weight: 500;
    white-space: nowrap;
  }
  .sub {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.8125rem;
    color: var(--muted);
  }
  .hint {
    padding: 0.9rem 1rem;
    font-size: 0.875rem;
    color: var(--muted);
  }
  .keys {
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
