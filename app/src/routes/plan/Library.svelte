<!-- The planner's library: one search across songs, scripture, decks and
     media, with filter chips. Click a row (or Enter from the search) to add
     it to the selected service, or drag it into the order to place it. A
     reference ("Rom 8:28") reads as a passage. Files and folders dropped
     on the panel are imported: songs and Bibles through ImportFiles,
     PowerPoint files as decks, images and video as media. -->
<script lang="ts">
  import { onMount } from "svelte";
  import { open as pickFiles } from "@tauri-apps/plugin-dialog";
  import {
    request,
    type Deck,
    type ItemKind,
    type MediaFile,
    type Service,
    type SongDatabase,
    type SongHit,
    type Translation,
  } from "../../lib/api";
  import { live, report } from "../../lib/state.svelte";
  import {
    byFilter,
    countKinds,
    importedCounts,
    isLink,
    mightBeReference,
    rank,
    newSongs,
    remember,
    sortByTitle,
    sortPaths,
    summarize,
    type Entry,
    type Filter,
  } from "../../lib/library";
  import { deckSourceLabel, mediaKind } from "../../lib/media";
  import { formatPassage, parseReference } from "../../lib/passage";
  import Button from "../../lib/kit/Button.svelte";
  import Icon from "../../lib/kit/Icon.svelte";
  import LibraryRow from "./LibraryRow.svelte";
  import SongDatabaseCard from "./SongDatabaseCard.svelte";

  type Item = NonNullable<Service["items"][string]>;

  let {
    service,
    items,
    decks,
    translations,
    onadd,
    ondragover,
    ondragend,
    onsongs,
    ondecks,
    ontranslations,
  }: {
    /** The service rows are added to, if one is selected. */
    service: Service | null;
    /** Its items, in order, for the "In service" checks. */
    items: [string, Item][];
    decks: Deck[];
    translations: Translation[];
    /** Adds at the end of the service; resolves true once it is in. */
    onadd: (kind: ItemKind) => Promise<boolean>;
    /** A row is carried over (x, y): true when that is over the service order. */
    ondragover: (x: number, y: number) => boolean;
    /** The carried row was let go: inserts it where the order shows, if over it. */
    ondragend: (kind: ItemKind | null) => Promise<boolean>;
    /** Songs a search found, so the order can name them. */
    onsongs: (hits: SongHit[]) => void;
    ondecks: () => Promise<void>;
    ontranslations: () => Promise<void>;
  } = $props();

  // Device-local preferences and history. Storage can be missing or full
  // (a private window), so every read and write is wrapped.
  function load<T>(key: string, fallback: T): T {
    try {
      const raw = localStorage.getItem(key);
      return raw ? (JSON.parse(raw) as T) : fallback;
    } catch {
      return fallback;
    }
  }
  function save(key: string, value: unknown) {
    try {
      localStorage.setItem(key, JSON.stringify(value));
    } catch {
      // Not remembered this time.
    }
  }

  const filters: { id: Filter; label: string }[] = [
    { id: "all", label: "All" },
    { id: "song", label: "Songs" },
    { id: "scripture", label: "Scripture" },
    { id: "media", label: "Media" },
    { id: "deck", label: "Decks" },
  ];
  /** Songs listed with no search; a search finds the rest. */
  const LISTED = 200;

  let query = $state("");
  let filter = $state<Filter>("all");
  let highlight = $state(0);
  let searchFocused = $state(false);
  let input: HTMLInputElement | undefined = $state();
  let panel: HTMLElement | undefined = $state();

  let hits: SongHit[] = $state([]);
  let allSongs: SongHit[] = $state([]);
  let media: MediaFile[] = $state([]);
  /**
   * Media dropped or picked here but not yet in any service. ListMedia
   * lists only media that services use, so these are kept on this device
   * until one is added.
   */
  let looseMedia: MediaFile[] = $state(load("lampstand.library.media", []));
  let recent: Entry[] = $state(load("lampstand.library.recent", []));
  let translation = $state(load("lampstand.library.translation", ""));
  let databases: SongDatabase[] = $state([]);
  let database = $state("en");

  let busy = $state(false);
  let downloading = $state(false);
  let dropping = $state(false);
  /** What the last import added, shown for a few seconds. */
  let notice = $state("");
  let addedKey: string | null = $state(null);
  let addedTimer: ReturnType<typeof setTimeout> | undefined;
  let noticeTimer: ReturnType<typeof setTimeout> | undefined;

  let trimmed = $derived(query.trim());
  let abbreviation = $derived(
    Object.fromEntries(translations.map((t) => [t.id, t.abbreviation || t.name])) as Record<string, string>,
  );

  // Keep the chosen translation one this device has.
  $effect(() => {
    if (translations.length && !translations.some((t) => t.id === translation)) translation = translations[0].id;
  });
  $effect(() => save("lampstand.library.translation", translation));

  // --- Search -------------------------------------------------------------

  // Each search and lookup is numbered so a slow answer to an old query
  // never replaces a newer one.
  let searchSeq = 0;
  async function search(q: string) {
    const seq = ++searchSeq;
    try {
      const found = await request({ area: "Library", request: { SearchSongs: { query: q, limit: q ? 50 : LISTED } } }, "Songs");
      if (seq !== searchSeq) return;
      if (q) hits = found;
      else {
        allSongs = found;
        hits = [];
      }
      onsongs(found);
    } catch (e) {
      report(e);
    }
  }

  $effect(() => {
    void live.songsVersion;
    const q = trimmed;
    search(q);
  });

  async function loadMedia() {
    try {
      media = await request({ area: "Library", request: "ListMedia" }, "MediaFiles");
    } catch (e) {
      report(e);
    }
  }

  // The reference the search reads as, and the core's lookup of it.
  let reference = $derived(mightBeReference(trimmed) ? parseReference(trimmed) : null);
  let passage: Entry | null = $state(null);
  let lookupSeq = 0;
  $effect(() => {
    const ref = reference;
    const t = translation;
    const seq = ++lookupSeq;
    if (!ref) {
      passage = null;
      return;
    }
    const title = formatPassage(ref);
    const key = `scripture:${JSON.stringify(ref)}:${t}`;
    if (!t) {
      passage = { key, kind: "scripture", title, subtitle: "No Bible on this device yet. Drop one here to import it.", item: null };
      return;
    }
    passage = { key, kind: "scripture", title, subtitle: "Looking up…", item: null };
    request({ area: "Library", request: { LookupPassage: { reference: trimmed, translation: t } } }, "Passage")
      .then((found) => {
        if (seq !== lookupSeq) return;
        const first = found.verses[0]?.[2] ?? "";
        passage = {
          key: `scripture:${JSON.stringify(found.passage)}:${t}`,
          kind: "scripture",
          title: found.display,
          subtitle: `${abbreviation[t] ?? ""} · ${first}`,
          item: { Scripture: { passage: found.passage, translation: t } },
        };
      })
      .catch((e) => {
        if (seq !== lookupSeq) return;
        passage = { key, kind: "scripture", title, subtitle: e instanceof Error ? e.message : String(e), item: null };
      });
  });

  // --- Entries -------------------------------------------------------------

  const songEntry = (h: SongHit): Entry => ({
    key: `song:${h.song}`,
    kind: "song",
    title: h.title,
    subtitle: "Song",
    snippet: h.line ? `“${h.line}”` : undefined,
    item: { Song: { song: h.song, arrangement: null } },
  });
  const deckEntry = (d: Deck): Entry => ({
    key: `deck:${d.id}`,
    kind: "deck",
    title: d.title,
    subtitle: `Slide deck · ${deckSourceLabel(d.source)}`,
    item: { Deck: { deck: d.id } },
  });
  const mediaEntry = (m: MediaFile): Entry => ({
    key: `media:${m.blob}`,
    kind: "media",
    title: m.name,
    subtitle: m.on_device ? mediaKind(m.name) : `${mediaKind(m.name)} · not on this device yet`,
    item: { Media: { blob: m.blob, name: m.name, looped: false } },
  });

  let allMedia = $derived([
    ...media,
    ...looseMedia.filter((m) => !media.some((n) => n.blob === m.blob)),
  ]);
  let others = $derived([...decks.map(deckEntry), ...allMedia.map(mediaEntry)]);
  let everything = $derived(sortByTitle([...allSongs.map(songEntry), ...others]));
  let libraryCount = $derived(allSongs.length + others.length);

  let linkEntry = $derived<Entry | null>(
    isLink(trimmed)
      ? { key: "link", kind: "deck", title: "Add deck from link", subtitle: trimmed, item: null }
      : null,
  );
  let results = $derived(
    trimmed ? [...(linkEntry ? [linkEntry] : []), ...rank(trimmed, passage, hits.map(songEntry), others)] : everything,
  );
  let counts = $derived(countKinds(results));

  // Recent rows (used in a service, or just added) that are still in the library.
  let recentShown = $derived(
    recent.filter(
      (e) =>
        (e.kind !== "deck" || decks.some((d) => e.key === `deck:${d.id}`)) &&
        (e.kind !== "scripture" || translations.length > 0),
    ),
  );
  /** Only the two seed hymns (or nothing): the library still needs filling. */
  let starter = $derived(allSongs.length <= 2 && others.length === 0);
  let showEmpty = $derived(!trimmed && filter === "all" && starter);
  let showRecent = $derived(!trimmed && filter === "all" && !starter && recentShown.length > 0);
  let showSongCard = $derived(!trimmed && filter === "song" && starter);
  let list = $derived(
    !trimmed && filter === "scripture" ? recentShown.filter((e) => e.kind === "scripture") : byFilter(results, filter),
  );
  /** Every row in the order the arrow keys walk them. */
  let rows = $derived([...(showRecent ? recentShown : []), ...list]);

  $effect(() => {
    void trimmed;
    void filter;
    highlight = 0;
  });
  $effect(() => {
    if (highlight >= rows.length) highlight = Math.max(0, rows.length - 1);
  });

  // In-service checks: songs, decks and media by ID, scripture by passage
  // and translation.
  let used = $derived(
    new Set(
      items.map(([, item]) => {
        const k = item.kind;
        if ("Song" in k) return `song:${k.Song.song}`;
        if ("Deck" in k) return `deck:${k.Deck.deck}`;
        if ("Media" in k) return `media:${k.Media.blob}`;
        if ("Scripture" in k) return `scripture:${JSON.stringify(k.Scripture.passage)}:${k.Scripture.translation}`;
        return "";
      }),
    ),
  );

  // --- Adding --------------------------------------------------------------

  function flash(entry: Entry) {
    addedKey = entry.key;
    clearTimeout(addedTimer);
    addedTimer = setTimeout(() => (addedKey = null), 1200);
    recent = remember(recent, { ...entry, snippet: undefined, fresh: false });
    save("lampstand.library.recent", recent);
  }

  async function add(entry: Entry) {
    if (entry.key === "link") return addLink(entry.subtitle);
    if (!entry.item) return;
    if (!service) {
      live.status = "Pick a service on the left first";
      return;
    }
    if (await onadd(entry.item)) flash(entry);
  }

  async function addLink(url: string) {
    busy = true;
    try {
      const deck = await request({ area: "Library", request: { AddDeckFromLink: { url } } }, "Deck");
      await ondecks();
      live.status = `Added ${deck.title}`;
      recent = remember(recent, { ...deckEntry(deck), fresh: true });
      save("lampstand.library.recent", recent);
      query = "";
      if (service) await onadd({ Deck: { deck: deck.id } });
    } catch (e) {
      report(e);
    } finally {
      busy = false;
    }
  }

  // --- Dragging a row into the order -----------------------------------------

  /** A row being carried: where it was pressed, and where it is now. */
  let carry: { entry: Entry; x0: number; y0: number; x: number; y: number; moving: boolean; over: boolean } | null =
    $state(null);
  /** A drag just ended on this row, so its click must not add it again. */
  let swallowClick = false;

  function press(e: PointerEvent, entry: Entry) {
    if (e.button !== 0 || !entry.item) return;
    // No text selection while carrying, and the search keeps its focus.
    e.preventDefault();
    carry = { entry, x0: e.clientX, y0: e.clientY, x: e.clientX, y: e.clientY, moving: false, over: false };
    const move = (m: PointerEvent) => {
      if (!carry) return;
      if (!carry.moving && Math.hypot(m.clientX - carry.x0, m.clientY - carry.y0) < 5) return;
      carry.moving = true;
      carry.x = m.clientX;
      carry.y = m.clientY;
      carry.over = ondragover(m.clientX, m.clientY);
    };
    const up = async (u: PointerEvent) => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", up);
      const c = carry;
      carry = null;
      if (!c?.moving) return;
      // The click that follows a drag must not add the row again.
      swallowClick = true;
      setTimeout(() => (swallowClick = false), 0);
      const drop = u.type === "pointerup" && c.over && !!service;
      if (await ondragend(drop ? $state.snapshot(c.entry.item) : null)) flash(c.entry);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", up);
  }

  function click(entry: Entry) {
    if (swallowClick) return;
    add(entry);
  }

  // --- Keys -------------------------------------------------------------------

  // `/` focuses the search unless the cursor is already in a field. The
  // service order's grips use Up and Down only while a grip has focus,
  // and the operator keys belong to Present, so nothing else here clashes.
  function onwindowkey(e: KeyboardEvent) {
    if (e.key !== "/" || e.metaKey || e.ctrlKey || e.altKey) return;
    const t = e.target as HTMLElement | null;
    if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT" || t.isContentEditable)) return;
    e.preventDefault();
    input?.focus();
  }

  function scrollTo(i: number) {
    document.getElementById(`library-row-${i}`)?.scrollIntoView({ block: "nearest" });
  }

  function onsearchkey(e: KeyboardEvent) {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      highlight = Math.min(highlight + 1, rows.length - 1);
      scrollTo(highlight);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      highlight = Math.max(highlight - 1, 0);
      scrollTo(highlight);
    } else if (e.key === "Enter") {
      e.preventDefault();
      const entry = rows[highlight];
      if (entry) add(entry);
    } else if (e.key === "Escape") {
      e.preventDefault();
      if (query) query = "";
      else input?.blur();
    }
  }

  function tryQuery(q: string) {
    filter = "all";
    query = q;
    input?.focus();
  }

  // --- Importing ----------------------------------------------------------------

  /** Shows what an import added ("3 songs · 1 deck") for a few seconds. */
  function announce(summary: string) {
    notice = summary.startsWith("Nothing") ? summary : `Added ${summary}`;
    clearTimeout(noticeTimer);
    noticeTimer = setTimeout(() => (notice = ""), 6000);
  }

  async function importPaths(paths: string[]) {
    const sorted = sortPaths(paths);
    if (!sorted.imports.length && !sorted.decks.length && !sorted.media.length) return;
    busy = true;
    const added = { songs: 0, bibles: 0, decks: 0, media: 0 };
    const failed: string[] = [];
    let message = "";
    const knownSongs = allSongs.map((s) => s.song);
    const fresh: Entry[] = [];
    try {
      if (sorted.imports.length) {
        message = await request({ area: "Library", request: { ImportFiles: { paths: sorted.imports } } }, "Text");
        Object.assign(added, importedCounts(message));
        // A few new songs are worth listing under Recent; a songbook is not.
        if (added.songs > 0) {
          const after = await request({ area: "Library", request: { SearchSongs: { query: "", limit: LISTED } } }, "Songs");
          fresh.push(...newSongs(knownSongs, after).map((h) => ({ ...songEntry(h), fresh: true })));
        }
      }
      for (const path of sorted.decks) {
        try {
          const deck = await request({ area: "Library", request: { AddDeckFile: { path } } }, "Deck");
          added.decks++;
          fresh.push({ ...deckEntry(deck), fresh: true });
        } catch (e) {
          failed.push(e instanceof Error ? e.message : String(e));
        }
      }
      for (const path of sorted.media) {
        live.status = "Copying into the library…";
        try {
          const file = await request({ area: "Library", request: { AddMediaFile: { path } } }, "Media");
          looseMedia = [...looseMedia.filter((m) => m.blob !== file.blob), file];
          added.media++;
          fresh.push({ ...mediaEntry(file), fresh: true });
        } catch (e) {
          failed.push(e instanceof Error ? e.message : String(e));
        }
      }
      save("lampstand.library.media", looseMedia);
      if (fresh.length > 0) {
        for (const entry of fresh) recent = remember(recent, entry);
        save("lampstand.library.recent", recent);
      }
      if (added.decks) await ondecks();
      if (added.bibles) await ontranslations();
      const summary = summarize(added);
      announce(failed.length ? `${summary} · ${failed.length} could not be added` : summary);

      // The status bar keeps the full line: what was skipped and why.
      live.status = [message, ...failed].filter(Boolean).join(". ") || summary;
    } catch (e) {
      report(e);
    } finally {
      busy = false;
    }
  }

  async function browse(directory = false) {
    try {
      const picked = await pickFiles(
        directory
          ? { directory: true, title: "Add a song folder or OpenSong library" }
          : { multiple: true, title: "Add songs, Bibles, decks or media" },
      );
      if (picked) await importPaths(Array.isArray(picked) ? picked : [picked]);
    } catch (e) {
      report(e);
    }
  }

  async function downloadDatabase() {
    busy = true;
    downloading = true;
    try {
      const message = await request({ area: "Library", request: { DownloadSongDatabase: { code: database } } }, "Text");
      live.status = message;
      announce(summarize(importedCounts(message)));
    } catch (e) {
      report(e);
    } finally {
      busy = false;
      downloading = false;
    }
  }

  function overPanel(position: { x: number; y: number }): boolean {
    if (!panel) return false;
    // Tauri gives physical pixels; the page works in CSS pixels.
    const x = position.x / devicePixelRatio;
    const y = position.y / devicePixelRatio;
    const r = panel.getBoundingClientRect();
    return x >= r.left && x <= r.right && y >= r.top && y <= r.bottom;
  }

  // ListMedia is every service's media, so it changes with the order.
  $effect(() => {
    void items;
    loadMedia();
  });

  onMount(() => {
    request({ area: "Library", request: "ListSongDatabases" }, "SongDatabases")
      .then((d) => (databases = d))
      .catch(report);

    // Files dropped on the window come as paths through Tauri, not as DOM
    // drop events (the webview keeps those for itself).
    let stop: (() => void) | undefined;
    let gone = false;
    import("@tauri-apps/api/webview")
      .then(({ getCurrentWebview }) =>
        getCurrentWebview().onDragDropEvent(({ payload }) => {
          if (payload.type === "leave") dropping = false;
          else if (payload.type === "enter" || payload.type === "over") dropping = overPanel(payload.position);
          else if (payload.type === "drop") {
            const here = overPanel(payload.position);
            dropping = false;
            if (here) importPaths(payload.paths);
          }
        }),
      )
      .then((unlisten) => {
        if (gone) unlisten();
        else stop = unlisten;
      })
      .catch(() => {
        // Not in Tauri (a plain browser): no file drops.
      });
    return () => {
      gone = true;
      stop?.();
      clearTimeout(addedTimer);
      clearTimeout(noticeTimer);
    };
  });
</script>

<svelte:window onkeydown={onwindowkey} />

{#snippet row(entry: Entry, i: number)}
  <LibraryRow
    {entry}
    id="library-row-{i}"
    active={searchFocused && i === highlight}
    added={addedKey === entry.key}
    inService={used.has(entry.key)}
    onadd={() => click(entry)}
    onhover={() => (highlight = i)}
    onpointerdown={(e) => press(e, entry)}
  />
{/snippet}

{#snippet dropZone()}
  <div class="drop-zone">
    <Icon name="upload" size={20} />
    <div>
      <p>Drop songs, Bibles, images, videos or slides here</p>
      <p class="muted small">Whole folders of songs work too. Everything is sorted automatically.</p>
    </div>
    <Button size="small" onclick={() => browse()} disabled={busy}>Browse files</Button>
  </div>
{/snippet}

<aside class="pane pane-side library" aria-label="Library" bind:this={panel}>
  <header class="head">
    <div class="title-row">
      <h2 class="title">Library{#if libraryCount > 0}<span class="count num">{libraryCount}</span>{/if}</h2>
      <div class="add">
        <Button size="small" icon="upload" onclick={() => browse()} disabled={busy} title="Add files, or drop them anywhere on the library">
          Add files
        </Button>
        <Button size="small" variant="ghost" icon="folder" onclick={() => browse(true)} disabled={busy} label="Add a folder" title="Add a song folder or OpenSong library" />
      </div>
    </div>

    <div class="search">
      <span class="search-icon"><Icon name="search" /></span>
      <input
        bind:this={input}
        bind:value={query}
        type="text"
        aria-label="Search the library"
        placeholder="Songs, lyrics, John 3:16, media…"
        onkeydown={onsearchkey}
        onfocus={() => (searchFocused = true)}
        onblur={() => (searchFocused = false)}
      />
      {#if query}
        <button type="button" class="clear" aria-label="Clear search" onclick={() => tryQuery("")}><Icon name="x" size={14} /></button>
      {:else}
        <kbd class="slash" aria-hidden="true">/</kbd>
      {/if}
    </div>

    <div class="chips-row">
      <div class="chips" role="group" aria-label="Filter by type">
        {#each filters as f (f.id)}
          <button type="button" class="chip" aria-pressed={filter === f.id} onclick={() => (filter = f.id)}>
            {f.label}{#if trimmed}<span class="chip-count num">{counts[f.id]}</span>{/if}
          </button>
        {/each}
      </div>
    </div>
    {#if translations.length > 0 && (passage || filter === "scripture")}
      <label class="translation">
        <span class="muted">Read from</span>
        <select title="The translation scripture is read from" bind:value={translation}>
          {#each translations as t (t.id)}
            <option value={t.id}>{t.abbreviation ? `${t.abbreviation} · ${t.name}` : t.name}</option>
          {/each}
        </select>
      </label>
    {/if}
    {#if notice}<p class="notice" role="status"><Icon name="check" size={14} />{notice}</p>{/if}
  </header>

  <div class="body">
    {#if showEmpty}
      <div class="empty-state">
        <SongDatabaseCard {databases} bind:code={database} {downloading} disabled={busy} ondownload={downloadDatabase} />
        {@render dropZone()}
        <p class="muted small center">
          Scripture is built in. Try
          <button type="button" class="link" onclick={() => tryQuery("John 3:16")}>John 3:16</button>
          or <button type="button" class="link" onclick={() => tryQuery("Ps 23")}>Ps 23</button>
        </p>
      </div>
    {/if}
    {#if showSongCard && allSongs.length === 0}
      <div class="empty-state">
        <SongDatabaseCard {databases} bind:code={database} {downloading} disabled={busy} ondownload={downloadDatabase} />
      </div>
    {:else}
      <div class="rows">
        {#if showSongCard}
          <div class="card-gap">
            <SongDatabaseCard {databases} bind:code={database} {downloading} disabled={busy} ondownload={downloadDatabase} />
          </div>
        {/if}
        {#if showRecent}
          <section aria-label="Recent" class="recent">
            <h3>Recent</h3>
            <ul class="plain">
              {#each recentShown as entry, i (entry.key)}{@render row(entry, i)}{/each}
            </ul>
          </section>
        {/if}
        {#if !trimmed && filter === "scripture"}
          <p class="hint muted">Type any reference, like “Rom 8:28” or “1 Cor 13”.</p>
          {#if translations.length === 0}
            <p class="hint muted">No Bibles on this device yet. Drop one here (OSIS, USFM, Zefania, OpenSong or OpenLP) to import it.</p>
          {:else if list.length > 0}
            <h3>Recent</h3>
          {/if}
        {/if}
        {#if showRecent || (showEmpty && list.length > 0)}<h3>Everything</h3>{/if}
        {#if list.length > 0}
          <ul class="plain">
            {#each list as entry, i (entry.key)}{@render row(entry, i + (showRecent ? recentShown.length : 0))}{/each}
          </ul>
          {#if !trimmed && filter !== "scripture" && filter !== "deck" && filter !== "media" && allSongs.length >= LISTED}
            <p class="hint muted">Showing the first {LISTED} songs. Search to find the rest.</p>
          {/if}
        {:else if trimmed}
          <div class="nothing">
            <p>No matches for “{trimmed}”</p>
            <p class="muted small">Drop files here or <button type="button" class="link" onclick={() => browse()}>browse</button> to add them.</p>
          </div>
        {:else if filter !== "scripture" && !showEmpty}
          {@render dropZone()}
        {/if}
      </div>
    {/if}
  </div>

  <footer class="foot">
    {#if service}
      Click or <kbd>Enter</kbd> adds to <span class="target">{service.title}</span> · drag to place
    {:else}
      Pick a service on the left to start adding
    {/if}
  </footer>

  {#if dropping}
    <div class="overlay" aria-hidden="true">
      <Icon name="upload" size={24} />
      <p><strong>Drop to add to your library</strong></p>
      <p class="muted small">Songs, Bibles, images, videos and slides are sorted automatically</p>
    </div>
  {/if}
</aside>

{#if carry?.moving}
  <div class="ghost" class:over={carry.over} style:left="{carry.x + 14}px" style:top="{carry.y + 10}px" aria-hidden="true">
    {carry.entry.title}
  </div>
{/if}

<style>
  .library {
    position: relative;
    border-left: 1px solid var(--line);
  }
  .head {
    display: flex;
    flex-direction: column;
    gap: 0.7rem;
    padding: 1.25rem 1rem 0.75rem;
  }
  .title-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
  }
  .count {
    margin-left: 0.5rem;
    font-weight: 400;
    color: var(--faint);
  }
  .add {
    display: flex;
    gap: 0.15rem;
  }
  .search {
    position: relative;
  }
  .search-icon {
    position: absolute;
    left: 0.7rem;
    top: 50%;
    transform: translateY(-50%);
    color: var(--faint);
    pointer-events: none;
  }
  .search input {
    width: 100%;
    box-sizing: border-box;
    padding-left: 2.2rem;
    padding-right: 2rem;
  }
  .clear,
  .slash {
    position: absolute;
    right: 0.5rem;
    top: 50%;
    transform: translateY(-50%);
  }
  .clear {
    background: none;
    border: none;
    color: var(--faint);
    padding: 0.2rem;
    border-radius: 4px;
    cursor: pointer;
    display: flex;
  }
  .clear:hover {
    color: var(--text);
  }
  kbd {
    font: inherit;
    font-size: 0.7rem;
    color: var(--faint);
    border: 1px solid var(--line);
    border-radius: 3px;
    padding: 0 0.3rem;
  }
  .slash {
    pointer-events: none;
  }
  .chips-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.4rem;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 0.1rem;
    min-width: 0;
  }
  .chip {
    font: inherit;
    font-size: 0.75rem;
    color: var(--muted);
    background: none;
    border: none;
    border-radius: 999px;
    padding: 0.2rem 0.45rem;
    cursor: pointer;
    white-space: nowrap;
  }
  .chip:hover {
    color: var(--text);
  }
  .chip[aria-pressed="true"] {
    background: var(--raised);
    color: var(--text);
  }
  .chip-count {
    margin-left: 0.25rem;
    color: var(--faint);
  }
  .translation {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.76rem;
  }
  .translation select {
    flex: 1;
    min-width: 0;
    font-size: 0.76rem;
    padding: 0.2rem 0.4rem;
  }
  .notice {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    font-size: 0.78rem;
    color: var(--ready-text);
  }
  .body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    border-top: 1px solid var(--line);
  }
  .rows {
    padding: 0.5rem;
  }
  .card-gap {
    padding: 0.25rem 0.25rem 0.75rem;
  }
  .recent {
    margin-bottom: 0.5rem;
    padding-bottom: 0.5rem;
    border-bottom: 1px solid var(--line);
  }
  h3 {
    padding: 0.3rem 0.6rem 0.25rem;
    font-size: 0.72rem;
    font-weight: 500;
    color: var(--faint);
  }
  .hint {
    padding: 0.3rem 0.6rem 0.5rem;
    font-size: 0.78rem;
  }
  .empty-state {
    display: flex;
    flex-direction: column;
    gap: 1rem;
    padding: 1rem;
  }
  .drop-zone {
    margin: 0.25rem;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.7rem;
    padding: 1.4rem 1rem;
    border: 1px dashed var(--line-strong);
    border-radius: var(--radius-lg);
    text-align: center;
    font-size: 0.875rem;
    color: var(--muted);
  }
  .empty-state .drop-zone {
    margin: 0;
  }
  .drop-zone p:first-child {
    color: var(--text);
  }
  .drop-zone p + p {
    margin-top: 0.2rem;
  }
  .small {
    font-size: 0.75rem;
  }
  .center {
    text-align: center;
  }
  .link {
    font: inherit;
    color: var(--gold);
    background: none;
    border: none;
    padding: 0;
    cursor: pointer;
  }
  .link:hover {
    text-decoration: underline;
    text-underline-offset: 2px;
  }
  .nothing {
    padding: 2.5rem 1rem;
    text-align: center;
    font-size: 0.875rem;
  }
  .nothing p + p {
    margin-top: 0.3rem;
  }
  .foot {
    border-top: 1px solid var(--line);
    padding: 0.6rem 1rem;
    font-size: 0.75rem;
    color: var(--faint);
  }
  .target {
    color: var(--muted);
  }
  .overlay {
    position: absolute;
    inset: 0.5rem;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.4rem;
    padding: 1rem;
    text-align: center;
    border: 2px dashed var(--gold);
    border-radius: var(--radius-lg);
    background: color-mix(in srgb, var(--canvas) 94%, transparent);
    color: var(--gold);
    font-size: 0.875rem;
    pointer-events: none;
  }
  .overlay p {
    color: var(--text);
  }
  .overlay p.muted {
    color: var(--muted);
    max-width: 15rem;
  }
  .ghost {
    position: fixed;
    z-index: 50;
    max-width: 16rem;
    padding: 0.35rem 0.6rem;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius);
    background: var(--raised);
    font-size: 0.8rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    pointer-events: none;
    box-shadow: 0 6px 18px rgba(0, 0, 0, 0.35);
  }
  .ghost.over {
    border-color: var(--gold);
  }
</style>
