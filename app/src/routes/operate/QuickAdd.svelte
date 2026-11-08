<!-- Quick add (Present): type a song title or a reference ("Rom 8:28") and
     put it in the service that is open for live, without going back to Plan.
     Enter adds it right after what is on air; Shift+Enter adds it and puts
     it on air now; Ctrl or Cmd+Enter adds it at the end. Songs and scripture
     only; decks and media are added in Plan. -->
<script lang="ts">
  import { onMount } from "svelte";
  import { request, type ItemKind, type SongHit, type Translation } from "../../lib/api";
  import { mightBeReference } from "../../lib/library";
  import { formatPassage, parseReferenceVersioned } from "../../lib/passage";
  import { chooseTranslation } from "../../lib/bibles";
  import { loadDefaultTranslation, saveDefaultTranslation } from "../../lib/bibleSetting";
  import { entryAction, insertionIndex, newItemId } from "../../lib/quickAdd";
  import { report } from "../../lib/state.svelte";
  import Icon from "../../lib/kit/Icon.svelte";

  let {
    service,
    order,
    liveItem,
    onclose,
    onadded,
  }: {
    /** The service open for live. */
    service: string;
    /** Its items, in order, for finding where "next" is. */
    order: string[];
    /** The item on air, or null. */
    liveItem: string | null;
    onclose: () => void;
    /** An item went in: its ID (null if it could not be found), whether to put it on air, and its title. */
    onadded: (id: string | null, show: boolean, title: string) => void;
  } = $props();

  interface Row {
    key: string;
    kind: "song" | "scripture";
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
  let trimmed = $derived(query.trim());

  const labelOf = (id: string) => {
    const t = translations.find((t) => t.id === id);
    return t?.abbreviation || t?.name || "";
  };
  let abbreviation = $derived(translations.find((t) => t.id === translation)?.abbreviation || translations.find((t) => t.id === translation)?.name || "");

  onMount(() => {
    input?.focus();
    Promise.all([request({ area: "Library", request: "ListTranslations" }, "Translations"), loadDefaultTranslation()])
      .then(([all, preferred]) => {
        translations = [...all].sort((a, b) => a.name.localeCompare(b.name));
        translation = chooseTranslation(translations, preferred, translation);
      })
      .catch(report);
    const previous = document.activeElement as HTMLElement | null;
    return () => previous?.focus?.({ preventScroll: true });
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

  let rows: Row[] = $derived([
    ...(passage ? [passage] : []),
    ...songs.map<Row>((h) => ({
      key: `song:${h.song}`,
      kind: "song",
      title: h.title,
      subtitle: h.line ? `“${h.line}”` : "Song",
      item: { Song: { song: h.song, arrangement: null } },
    })),
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
    if (e.key === "Escape") {
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
  <div class="palette" role="dialog" aria-modal="true" aria-label="Quick add" tabindex="-1" onclick={(e) => e.stopPropagation()}>
    <div class="search">
      <Icon name="search" size={16} />
      <input
        bind:this={input}
        bind:value={query}
        {onkeydown}
        type="text"
        placeholder="Song title or reference, like Rom 8:28"
        autocomplete="off"
        spellcheck="false"
        aria-label="Song or scripture to add"
        aria-controls="quick-add-results"
      />
      {#if translations.length > 0 && passage}<span class="tag" title="Tab changes Bible">{abbreviation}</span>{/if}
    </div>

    <ul id="quick-add-results" class="plain results" role="listbox" aria-label="Results">
      {#each rows as row, i (row.key)}
        <li role="option" aria-selected={i === highlight}>
          <button type="button" class="row" class:active={i === highlight} disabled={!row.item} onmouseenter={() => (highlight = i)} onclick={(e) => add(row, e)}>
            <Icon name={row.kind === "song" ? "song" : "scripture"} size={16} />
            <span class="title">{row.title}</span>
            <span class="sub">{row.subtitle}</span>
          </button>
        </li>
      {:else}
        <li class="hint">
          {trimmed ? "Nothing found." : "Type a song title, or a reference like John 3:16."}
        </li>
      {/each}
    </ul>

    <p class="keys faint">
      <kbd>Enter</kbd> add next · <kbd>⇧ Enter</kbd> add and show now · <kbd>⌃/⌘ Enter</kbd> add at end{#if translations.length > 1} · <kbd>Tab</kbd> Bible{/if} · <kbd>Esc</kbd> close
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
