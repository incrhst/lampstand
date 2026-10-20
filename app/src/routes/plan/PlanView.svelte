<!-- Service planning: services down the left, the selected service's
     order in the middle, the song, deck and media library on the right.
     T49 replaces this with the collaborative planner. -->
<script lang="ts">
  import { untrack } from "svelte";
  import { open as pickFiles } from "@tauri-apps/plugin-dialog";
  import {
    request,
    type Deck,
    type ItemKind,
    type MediaFile,
    type Service,
    type ServiceSummary,
    type SongDatabase,
    type SongHit,
  } from "../../lib/api";
  import { live, report } from "../../lib/state.svelte";
  import Banner from "../../lib/kit/Banner.svelte";
  import Button from "../../lib/kit/Button.svelte";
  import Icon from "../../lib/kit/Icon.svelte";
  import ListRow from "../../lib/kit/ListRow.svelte";
  import Placeholder from "../../lib/kit/Placeholder.svelte";
  import Segmented from "../../lib/kit/Segmented.svelte";
  import { kindIcon, kindLabel, kindName } from "../../lib/kit/items";
  import { deckExtensions, deckSourceLabel, imageExtensions, isVideo, mediaKind, onePath, videoExtensions } from "../../lib/media";
  import DeckLibrary from "./DeckLibrary.svelte";
  import MediaLibrary from "./MediaLibrary.svelte";
  import { comingSunday, deviceZone, formatWhen, fromInputs, timeZones, toInputs } from "../../lib/when";

  let { onopened }: { onopened: () => void } = $props();

  let services: ServiceSummary[] = $state([]);
  let selected: Service | null = $state(null);
  let creating = $state(false);
  let newTitle = $state("Sunday service");
  /** A date and time as the inputs hold them, in an IANA zone. */
  type When = { date: string; time: string; zone: string };
  /** The coming Sunday at 10:00 in `zone`, as the inputs hold it. */
  const defaultWhen = (zone = deviceZone()): When => ({ ...toInputs(comingSunday(Date.now(), zone), zone), zone });
  /** The New service form's date, time and zone. */
  let newWhen: When = $state(defaultWhen());
  /** The header's date, time and zone while the planner changes them. */
  let editWhen: When | null = $state(null);
  const at = (w: When) => fromInputs(w.date, w.time, w.zone);
  let query = $state("");
  let hits: SongHit[] = $state([]);
  /** Title and author of every song we have looked up, by song ID. */
  let songInfo: Record<string, { title: string; author: string | null }> = $state({});
  let databases: SongDatabase[] = $state([]);
  let database = $state("en");
  let busy = $state(false);
  let getSongsOpen = $state(false);
  /** Songs already looked up with GetSong, so each is fetched once. */
  const lookedUp = new Set<string>();

  const libraryTabs = [
    { id: "songs", label: "Songs" },
    { id: "scripture", label: "Scripture", disabled: "Bible search in the planner arrives with T48" },
    { id: "media", label: "Media" },
    { id: "decks", label: "Decks" },
  ] as const;
  let libraryTab: (typeof libraryTabs)[number]["id"] = $state("songs");
  let decks: Deck[] = $state([]);
  let media: MediaFile[] = $state([]);
  let deckById = $derived(Object.fromEntries(decks.map((d) => [d.id, d])) as Record<string, Deck>);

  async function refresh() {
    try {
      services = await request({ area: "Service", request: "List" }, "Services");
    } catch (e) {
      report(e);
    }
  }

  function toggleCreate() {
    creating = !creating;
    // Each new service starts on the coming Sunday unless the planner
    // picks another day.
    if (creating) newWhen = defaultWhen();
  }

  async function create() {
    try {
      selected = await request(
        {
          area: "Service",
          request: {
            Create: {
              title: newTitle.trim() || "Service",
              starts_at: at(newWhen),
              time_zone: newWhen.zone,
            },
          },
        },
        "Service",
      );
      live.planning = selected.id;
      creating = false;
      await refresh();
    } catch (e) {
      report(e);
    }
  }

  async function reschedule(when: When | null) {
    if (!selected) return;
    const startsAt = when && at(when);
    try {
      selected = await request(
        {
          area: "Service",
          request: {
            Reschedule: {
              service: selected.id,
              starts_at: startsAt,
              time_zone: startsAt == null ? null : when!.zone,
            },
          },
        },
        "Service",
      );
      editWhen = null;
      await refresh();
    } catch (e) {
      report(e);
    }
  }

  const saveWhen = () => editWhen && reschedule(editWhen);
  const cancelWhen = () => (editWhen = null);

  /**
   * Enter submits and Escape cancels from any field. The kit's Button is
   * `type="button"`, so a form of several fields has no implicit submit.
   */
  function onKeys(submit: () => void, cancel?: () => void) {
    return (e: KeyboardEvent) => {
      if (e.key === "Enter") {
        e.preventDefault();
        submit();
      } else if (e.key === "Escape" && cancel) {
        cancel();
      }
    };
  }

  function startReschedule() {
    if (!selected) return;
    const s = selected;
    editWhen =
      s.starts_at == null
        ? defaultWhen(s.time_zone ?? deviceZone())
        : { ...toInputs(s.starts_at, s.time_zone ?? deviceZone()), zone: s.time_zone ?? deviceZone() };
  }

  async function select(id: string) {
    editWhen = null;
    try {
      selected = await request({ area: "Service", request: { Get: id } }, "Service");
      live.planning = id;
    } catch (e) {
      report(e);
    }
  }

  async function search() {
    try {
      hits = await request(
        { area: "Library", request: { SearchSongs: { query, limit: 20 } } },
        "Songs",
      );
      for (const h of hits) songInfo[h.song] ??= { title: h.title, author: null };
    } catch (e) {
      report(e);
    }
  }

  // Look up songs in the order that a search has not brought in, and
  // every song's author, which search hits do not carry.
  async function lookUpSongs(service: Service) {
    for (const [, item] of ordered(service)) {
      const k = item.kind;
      if (!("Song" in k)) continue;
      const id = k.Song.song;
      if (lookedUp.has(id)) continue;
      lookedUp.add(id);
      try {
        const song = await request({ area: "Library", request: { GetSong: id } }, "Song");
        songInfo[id] = { title: song.title, author: song.author };
      } catch {
        // A song missing from the library shows as "Song".
      }
    }
  }

  /** Adds an item at the end of the selected service. */
  async function addItem(kind: ItemKind) {
    if (!selected) return;
    try {
      selected = await request(
        {
          area: "Service",
          request: {
            InsertItem: {
              service: selected.id,
              index: Object.keys(selected.items).length,
              kind,
            },
          },
        },
        "Service",
      );
    } catch (e) {
      report(e);
    }
  }

  const addSong = (song: string) => addItem({ Song: { song, arrangement: null } });

  async function loadDecks() {
    try {
      decks = await request({ area: "Library", request: "ListDecks" }, "Decks");
    } catch (e) {
      report(e);
    }
  }

  async function loadMedia() {
    try {
      media = await request({ area: "Library", request: "ListMedia" }, "MediaFiles");
    } catch (e) {
      report(e);
    }
  }

  /** Puts a newly added deck in the library list and the service. */
  async function deckAdded(deck: Deck) {
    live.status = `Added ${deck.title}`;
    await loadDecks();
    await addItem({ Deck: { deck: deck.id } });
  }

  // Parsing checks the deck, so a big one takes a moment.
  async function pickDeck() {
    try {
      const path = onePath(
        await pickFiles({
          title: "Add a PowerPoint deck",
          filters: [{ name: "PowerPoint", extensions: deckExtensions }],
        }),
      );
      if (!path) return;
      busy = true;
      await deckAdded(await request({ area: "Library", request: { AddDeckFile: { path } } }, "Deck"));
    } catch (e) {
      report(e);
    } finally {
      busy = false;
    }
  }

  async function addDeckLink(url: string): Promise<boolean> {
    busy = true;
    try {
      await deckAdded(await request({ area: "Library", request: { AddDeckFromLink: { url } } }, "Deck"));
      return true;
    } catch (e) {
      report(e);
      return false;
    } finally {
      busy = false;
    }
  }

  // Video is copied into the library, so a long one takes a moment.
  async function pickMedia() {
    if (!selected) return;
    try {
      const path = onePath(
        await pickFiles({
          title: "Add a video or image",
          filters: [
            { name: "Video and images", extensions: [...videoExtensions, ...imageExtensions] },
            { name: "Video", extensions: videoExtensions },
            { name: "Images", extensions: imageExtensions },
          ],
        }),
      );
      if (!path) return;
      busy = true;
      live.status = "Copying into the library…";
      const file = await request({ area: "Library", request: { AddMediaFile: { path } } }, "Media");
      await addMedia(file);
      live.status = `Added ${file.name}`;
    } catch (e) {
      report(e);
    } finally {
      busy = false;
    }
  }

  async function addMedia(file: MediaFile) {
    await addItem({ Media: { blob: file.blob, name: file.name, looped: false } });
    await loadMedia();
  }

  async function remove(item: string) {
    if (!selected) return;
    try {
      selected = await request(
        { area: "Service", request: { RemoveItem: { service: selected.id, item } } },
        "Service",
      );
    } catch (e) {
      report(e);
    }
  }

  async function move(item: string, index: number) {
    if (!selected) return;
    try {
      selected = await request(
        { area: "Service", request: { MoveItem: { service: selected.id, item, index } } },
        "Service",
      );
    } catch (e) {
      report(e);
    }
  }

  async function openService() {
    if (!selected) return;
    try {
      live.loaded = await request({ area: "Service", request: { Open: selected.id } }, "Loaded");
      onopened();
    } catch (e) {
      report(e);
    }
  }

  async function importPaths(paths: string[]) {
    busy = true;
    try {
      live.status = await request({ area: "Library", request: { ImportFiles: { paths } } }, "Text");
      await search();
    } catch (e) {
      report(e);
    } finally {
      busy = false;
    }
  }

  async function importFiles() {
    try {
      const picked = await pickFiles({ multiple: true, title: "Import songs or Bibles" });
      if (picked) await importPaths(Array.isArray(picked) ? picked : [picked]);
    } catch (e) {
      report(e);
    }
  }

  // Bring your own OpenSong library: its data folder or its Songs folder.
  async function importFolder() {
    try {
      const picked = await pickFiles({ directory: true, title: "Import an OpenSong library or song folder" });
      if (picked) await importPaths(Array.isArray(picked) ? picked : [picked]);
    } catch (e) {
      report(e);
    }
  }

  async function downloadDatabase() {
    busy = true;
    try {
      live.status = await request(
        { area: "Library", request: { DownloadSongDatabase: { code: database } } },
        "Text",
      );
      await search();
    } catch (e) {
      report(e);
    } finally {
      busy = false;
    }
  }

  function ordered(service: Service) {
    return Object.entries(service.items)
      .filter((entry): entry is [string, NonNullable<(typeof entry)[1]>] => entry[1] != null)
      .sort(([a, x], [b, y]) =>
        x.position < y.position ? -1 : x.position > y.position ? 1 : a < b ? -1 : 1,
      );
  }

  type Item = NonNullable<Service["items"][string]>;

  function itemTitle(item: Item): string {
    const k = item.kind;
    if ("Song" in k) return songInfo[k.Song.song]?.title ?? "Song";
    if ("Header" in k) return k.Header.title;
    if ("Scripture" in k) return "Scripture";
    if ("Deck" in k) return deckById[k.Deck.deck]?.title ?? "Presentation";
    if ("Media" in k) return k.Media.name;
    if ("Countdown" in k) return k.Countdown.label;
    return "Announcements";
  }

  function itemSubtitle(item: Item): string {
    const k = item.kind;
    const label = kindLabel[kindName(k)];
    if ("Song" in k) {
      const author = songInfo[k.Song.song]?.author;
      return author ? `${label} · ${author}` : label;
    }
    if ("Scripture" in k) return `${label} · ${k.Scripture.translation}`;
    if ("Deck" in k) {
      const deck = deckById[k.Deck.deck];
      return deck ? `${label} · ${deckSourceLabel(deck.source)}` : label;
    }
    if ("Media" in k) return `${mediaKind(k.Media.name)}${k.Media.looped ? " · loops" : ""}`;
    return label;
  }

  let items = $derived(selected ? ordered(selected) : []);
  let inService = $derived(
    new Set(items.flatMap(([, item]) => ("Song" in item.kind ? [item.kind.Song.song] : []))),
  );
  let decksInService = $derived(
    new Set(items.flatMap(([, item]) => ("Deck" in item.kind ? [item.kind.Deck.deck] : []))),
  );
  let mediaInService = $derived(
    new Set(items.flatMap(([, item]) => ("Media" in item.kind ? [item.kind.Media.blob] : []))),
  );

  // Reordering: drag a row by its handle, or focus the handle and press
  // the up and down arrows. Pointer events rather than HTML drag and drop,
  // which the webview's own file-drop handling can swallow.
  let list: HTMLOListElement | undefined = $state();
  let dragging: { id: string; from: number } | null = $state(null);
  let dropAt: number | null = $state(null);

  function slotFor(y: number): number {
    if (!list || !dragging) return 0;
    const rows = [...list.querySelectorAll<HTMLElement>("[data-row]")];
    let slot = 0;
    rows.forEach((row, i) => {
      if (i === dragging!.from) return;
      const r = row.getBoundingClientRect();
      if (y > r.top + r.height / 2) slot++;
    });
    return slot;
  }

  function startDrag(e: PointerEvent, id: string, from: number) {
    if (e.button !== 0) return;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    dragging = { id, from };
    dropAt = from;
  }

  function dragMove(e: PointerEvent) {
    if (dragging) dropAt = slotFor(e.clientY);
  }

  function endDrag() {
    if (dragging && dropAt !== null && dropAt !== dragging.from) move(dragging.id, dropAt);
    dragging = null;
    dropAt = null;
  }

  function handleKey(e: KeyboardEvent, id: string, index: number) {
    if (e.key === "ArrowUp" && index > 0) {
      e.preventDefault();
      move(id, index - 1);
    } else if (e.key === "ArrowDown" && index < items.length - 1) {
      e.preventDefault();
      move(id, index + 1);
    }
  }

  /** Where the drop line shows: above row `i`, or after the last row. */
  function lineBefore(i: number): boolean {
    if (!dragging || dropAt === null || dropAt === dragging.from) return false;
    // `dropAt` counts rows other than the dragged one.
    const visual = dropAt >= dragging.from ? dropAt + 1 : dropAt;
    return visual === i;
  }

  async function loadDatabases() {
    try {
      databases = await request({ area: "Library", request: "ListSongDatabases" }, "SongDatabases");
    } catch (e) {
      report(e);
    }
  }

  // Refetch songs after an import finishes elsewhere.
  $effect(() => {
    void live.songsVersion;
    search();
  });

  $effect(() => {
    const service = selected;
    if (service) untrack(() => lookUpSongs(service));
  });

  // Refetch decks when one is added or synced in.
  $effect(() => {
    void live.decksVersion;
    loadDecks();
  });

  $effect(() => {
    if (libraryTab === "media") untrack(loadMedia);
  });

  refresh();
  loadDatabases();
  if (live.planning) select(live.planning);
</script>

{#snippet zonePicker(when: When)}
  <select class="zone" aria-label="Time zone" title="The time zone this service's time is in" bind:value={when.zone}>
    {#each timeZones(when.zone, deviceZone()) as zone (zone)}
      <option value={zone}>{zone.replaceAll("_", " ")}{zone === deviceZone() ? " (this device)" : ""}</option>
    {/each}
  </select>
{/snippet}

<div class="plan">
  <aside class="pane pane-side services" aria-label="Services">
    <div class="pane-head">
      <h2>Services</h2>
      <Button size="small" icon="plus" onclick={toggleCreate} pressed={creating}>New service</Button>
    </div>
    {#if creating}
      <form
        class="create"
        onsubmit={(e) => {
          e.preventDefault();
          create();
        }}
      >
        <!-- svelte-ignore a11y_autofocus -->
        <input aria-label="New service title" bind:value={newTitle} onkeydown={onKeys(create)} autofocus />
        <div class="when">
          <input type="date" aria-label="Service date" bind:value={newWhen.date} onkeydown={onKeys(create)} />
          <input type="time" aria-label="Service time" bind:value={newWhen.time} onkeydown={onKeys(create)} />
        </div>
        {@render zonePicker(newWhen)}
        <Button variant="primary" size="small" onclick={create}>Create</Button>
      </form>
    {/if}
    <ul class="plain pane-body">
      {#each services as s (s.id)}
        <ListRow title={s.title} subtitle={formatWhen(s.starts_at, s.time_zone)} active={selected?.id === s.id} onclick={() => select(s.id)}>
          {#snippet details()}
            {#if live.loaded?.service === s.id}
              <span class="row-status"><span class="live-dot"></span>Open for live</span>
            {/if}
          {/snippet}
        </ListRow>
      {:else}
        <li class="empty muted">No services yet.</li>
      {/each}
    </ul>
  </aside>

  <section class="pane order" aria-labelledby="service-title">
    {#if selected}
      <div class="order-head">
        <div class="heading">
          <h1 id="service-title">{selected.title}</h1>
          {#if editWhen}
            <form
              class="reschedule"
              onsubmit={(e) => e.preventDefault()}
            >
              <input type="date" aria-label="Service date" bind:value={editWhen.date} onkeydown={onKeys(saveWhen, cancelWhen)} />
              <input type="time" aria-label="Service time" bind:value={editWhen.time} onkeydown={onKeys(saveWhen, cancelWhen)} />
              {@render zonePicker(editWhen)}
              <Button variant="primary" size="small" onclick={saveWhen}>Save</Button>
              <Button variant="ghost" size="small" onclick={() => reschedule(null)} disabled={selected.starts_at == null}>Clear date</Button>
              <Button variant="ghost" size="small" icon="x" label="Cancel" onclick={cancelWhen} />
                  </form>
          {:else}
            <p class="muted when-line">
              <span>
                {formatWhen(selected.starts_at, selected.time_zone)} · {items.length}
                {items.length === 1 ? "item" : "items"}
              </span>
              <Button variant="ghost" size="small" icon="calendar" label="Change date and time" title="Change date and time" onclick={startReschedule} />
            </p>
          {/if}
        </div>
        <div class="actions">
          <Button icon="deck" onclick={pickDeck} disabled={busy}>Add deck</Button>
          <Button icon="media" onclick={pickMedia} disabled={busy}>Add media</Button>
          <Button variant="primary" icon="play" onclick={openService} disabled={items.length === 0}>Open for live</Button>
        </div>
      </div>

      {#if items.length > 0}
        <div class="banner">
          <Banner>
            <span>Nothing has checked that every item is on this device yet.</span>
            <Placeholder ticket="T36">Readiness</Placeholder>
          </Banner>
        </div>
      {/if}

      <div class="order-body">
        {#if items.length === 0}
          <div class="empty-order">
            <p><strong>This service is empty</strong></p>
            <p class="muted">Add songs, decks and media from the library on the right.</p>
          </div>
        {:else}
          <ol class="plain items" bind:this={list} aria-label="Service order">
            {#each items as [id, item], i (id)}
              <li data-row class="item" class:dragging={dragging?.id === id} class:line-before={lineBefore(i)}>
                <button
                  type="button"
                  class="grip"
                  aria-label="Move {itemTitle(item)}. Drag, or use the up and down arrows."
                  onpointerdown={(e) => startDrag(e, id, i)}
                  onpointermove={dragMove}
                  onpointerup={endDrag}
                  onpointercancel={endDrag}
                  onkeydown={(e) => handleKey(e, id, i)}
                >
                  <Icon name="grip" />
                </button>
                <span class="index num">{i + 1}</span>
                <span class="kind"><Icon name={kindIcon[kindName(item.kind)]} label={kindLabel[kindName(item.kind)]} /></span>
                <span class="text">
                  <span class="title">{itemTitle(item)}</span>
                  <span class="subtitle">{itemSubtitle(item)}</span>
                </span>
                {#if "Media" in item.kind && isVideo(item.kind.Media.name)}
                  <Placeholder ticket="T27">Not on screen yet</Placeholder>
                {/if}
                <span class="remove">
                  <Button variant="ghost" size="small" icon="trash" label="Remove {itemTitle(item)}" onclick={() => remove(id)} />
                </span>
              </li>
            {/each}
            {#if lineBefore(items.length)}<li class="line-end" aria-hidden="true"></li>{/if}
          </ol>
        {/if}
      </div>
    {:else}
      <div class="no-service">
        <Icon name="calendar" size={24} />
        <p class="muted">Pick a service on the left, or create a new one.</p>
      </div>
    {/if}
  </section>

  <aside class="pane pane-side library" aria-label="Library">
    <div class="library-head">
      <h2 class="title">Library</h2>
      <Segmented options={libraryTabs} value={libraryTab} onchange={(id) => (libraryTab = id)} label="Library type" stretch />
      {#if libraryTab === "songs"}
      <form
        class="search"
        onsubmit={(e) => {
          e.preventDefault();
          search();
        }}
      >
        <span class="search-icon"><Icon name="search" /></span>
        <input type="search" aria-label="Search songs" placeholder="Search titles and lyrics" bind:value={query} oninput={search} />
      </form>
      {/if}
    </div>

    {#if libraryTab === "decks"}
      <DeckLibrary
        {decks}
        inService={decksInService}
        canAdd={!!selected}
        {busy}
        onadd={(deck) => addItem({ Deck: { deck } })}
        onpick={pickDeck}
        onlink={addDeckLink}
      />
    {:else if libraryTab === "media"}
      <MediaLibrary {media} inService={mediaInService} canAdd={!!selected} {busy} onadd={addMedia} onpick={pickMedia} />
    {:else}
    <ul class="plain pane-body">
      {#each hits as hit (hit.song)}
        <ListRow title={hit.title}>
          {#snippet details()}
            {#if hit.line}<span class="snippet">“{hit.line}”</span>{/if}
          {/snippet}
          {#snippet trailing()}
            {#if inService.has(hit.song)}
              <span class="in-service"><Icon name="check" size={14} />In service</span>
            {:else}
              <Button size="small" icon="plus" onclick={() => addSong(hit.song)} disabled={!selected} label="Add {hit.title} to service">
                Add
              </Button>
            {/if}
          {/snippet}
        </ListRow>
      {:else}
        <li class="empty muted">{query ? `Nothing matches “${query}”.` : "No songs yet. Get some below."}</li>
      {/each}
    </ul>

    <div class="get-songs">
      <button class="get-toggle" aria-expanded={getSongsOpen || hits.length <= 2} onclick={() => (getSongsOpen = !getSongsOpen)}>
        <Icon name="download" size={14} />
        Get songs
      </button>
      {#if getSongsOpen || hits.length <= 2}
        <p class="muted">
          Download a free song database from worshipleaderapp.com (OpenSong format, updated daily).
          Many songs are under copyright: your church's licence (CCLI or similar) covers projecting them.
          Songs you already have are not added twice.
        </p>
        <div class="row">
          <select aria-label="Song database language" bind:value={database}>
            {#each databases as d (d.code)}
              <option value={d.code}>{d.name}</option>
            {/each}
          </select>
          <Button variant="primary" size="small" onclick={downloadDatabase} disabled={busy}>
            {busy ? "Working…" : "Download and import"}
          </Button>
        </div>
      {/if}
      <div class="row">
        <Button size="small" icon="file" onclick={importFiles} disabled={busy}>Import files…</Button>
        <Button size="small" icon="folder" onclick={importFolder} disabled={busy}>Import folder…</Button>
      </div>
    </div>
    {/if}
  </aside>
</div>

<style>
  .plan {
    display: grid;
    grid-template-columns: 248px minmax(0, 1fr) 340px;
    height: 100%;
  }
  .services {
    border-right: 1px solid var(--line);
  }
  .library {
    border-left: 1px solid var(--line);
  }
  .create {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    padding: 0 1rem 0.75rem;
  }
  .create input,
  .reschedule input {
    min-width: 0;
    padding: 0.3rem 0.5rem;
  }
  .when {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 6.5rem;
    gap: 0.4rem;
  }
  .zone {
    min-width: 0;
    padding: 0.3rem 0.5rem;
    font-size: 0.8rem;
    color: var(--muted);
  }
  .reschedule .zone {
    max-width: 12rem;
  }
  .create > :global(button) {
    align-self: flex-end;
  }
  .row-status {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    margin-top: 0.3rem;
    font-size: 0.75rem;
    color: var(--muted);
  }
  .live-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--live);
  }
  .empty {
    padding: 2rem 0.75rem;
    text-align: center;
    font-size: 0.875rem;
  }

  /* Service order */
  .order-head {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-start;
    justify-content: space-between;
    gap: 1rem;
    padding: 1.5rem 2rem 1.25rem;
  }
  .heading {
    min-width: 0;
  }
  h1 {
    font-size: 1.3rem;
    font-weight: 600;
    letter-spacing: -0.01em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .heading p {
    margin-top: 0.25rem;
    font-size: 0.875rem;
  }
  .when-line {
    display: flex;
    align-items: center;
    gap: 0.25rem;
  }
  .reschedule {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.4rem;
    margin-top: 0.4rem;
  }
  .reschedule input {
    font-size: 0.85rem;
  }
  .actions {
    display: flex;
    gap: 0.5rem;
  }
  .banner {
    padding: 0 2rem;
  }
  .order-body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 1rem 2rem 2rem;
  }
  .item {
    position: relative;
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding: 0.75rem 0.5rem;
    border-bottom: 1px solid var(--line);
  }
  .item:last-of-type {
    border-bottom: none;
  }
  .item.dragging {
    opacity: 0.5;
  }
  .item.line-before::before,
  .line-end {
    content: "";
    position: absolute;
    left: 0;
    right: 0;
    top: -1px;
    height: 2px;
    background: var(--gold);
    border-radius: 2px;
  }
  .line-end {
    position: relative;
    display: block;
  }
  .grip {
    background: none;
    border: none;
    color: var(--faint);
    width: 1.25rem;
    height: 2rem;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 4px;
    cursor: grab;
    touch-action: none;
  }
  .grip:hover {
    color: var(--muted);
  }
  .grip:active {
    cursor: grabbing;
  }
  .index {
    width: 1.25rem;
    text-align: right;
    font-size: 0.75rem;
    color: var(--faint);
  }
  .kind {
    width: 2rem;
    height: 2rem;
    border-radius: var(--radius);
    background: var(--raised);
    color: var(--muted);
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }
  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .title {
    font-size: 0.9rem;
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .subtitle {
    font-size: 0.75rem;
    color: var(--muted);
  }
  .remove {
    opacity: 0;
    transition: opacity 150ms var(--ease);
  }
  .item:hover .remove,
  .remove:focus-within {
    opacity: 1;
  }
  .empty-order {
    border: 1px dashed var(--line-strong);
    border-radius: var(--radius-lg);
    padding: 4rem 1.5rem;
    text-align: center;
    font-size: 0.875rem;
  }
  .empty-order p + p {
    margin-top: 0.25rem;
  }
  .no-service {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.75rem;
    color: var(--faint);
    font-size: 0.875rem;
  }

  /* Library */
  .library-head {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    padding: 1.25rem 1rem 0.75rem;
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
  }
  .snippet {
    margin-top: 0.2rem;
    font-size: 0.75rem;
    font-style: italic;
    color: var(--faint);
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .in-service {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    height: 1.75rem;
    padding: 0 0.4rem;
  }
  .in-service :global(.icon) {
    color: var(--ready-text);
  }
  .get-songs {
    border-top: 1px solid var(--line);
    padding: 0.75rem 1rem 1rem;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }
  .get-toggle {
    font: inherit;
    font-size: 0.85rem;
    font-weight: 600;
    color: var(--text);
    background: none;
    border: none;
    padding: 0;
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    cursor: pointer;
    align-self: flex-start;
  }
  .get-songs p {
    font-size: 0.78rem;
    line-height: 1.5;
  }
  .row {
    display: flex;
    gap: 0.5rem;
    align-items: center;
  }
  .row select {
    flex: 1;
    min-width: 0;
    padding: 0.3rem 0.5rem;
  }
</style>
