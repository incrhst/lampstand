<!-- Bare service planning: list, create, add songs by search, open.
     T49 replaces this with the collaborative planner. -->
<script lang="ts">
  import { open as pickFiles } from "@tauri-apps/plugin-dialog";
  import {
    request,
    type Service,
    type ServiceSummary,
    type SongDatabase,
    type SongHit,
  } from "../../lib/api";
  import { live, report } from "../../lib/state.svelte";
  import Button from "../../lib/kit/Button.svelte";
  import Panel from "../../lib/kit/Panel.svelte";

  let { onopened }: { onopened: () => void } = $props();

  let services: ServiceSummary[] = $state([]);
  let selected: Service | null = $state(null);
  let newTitle = $state("Sunday service");
  let query = $state("");
  let hits: SongHit[] = $state([]);
  let songTitles: Record<string, string> = $state({});
  let databases: SongDatabase[] = $state([]);
  let database = $state("en");
  let busy = $state(false);

  async function refresh() {
    try {
      services = await request({ area: "Service", request: "List" }, "Services");
    } catch (e) {
      report(e);
    }
  }

  async function create() {
    try {
      selected = await request(
        { area: "Service", request: { Create: { title: newTitle.trim() || "Service" } } },
        "Service",
      );
      live.planning = selected.id;
      await refresh();
    } catch (e) {
      report(e);
    }
  }

  async function select(id: string) {
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
      for (const h of hits) songTitles[h.song] = h.title;
    } catch (e) {
      report(e);
    }
  }

  async function addSong(song: string) {
    if (!selected) return;
    try {
      selected = await request(
        {
          area: "Service",
          request: {
            InsertItem: {
              service: selected.id,
              index: Object.keys(selected.items).length,
              kind: { Song: { song, arrangement: null } },
            },
          },
        },
        "Service",
      );
    } catch (e) {
      report(e);
    }
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

  function itemTitle(kind: Service["items"][string]): string {
    if (!kind) return "";
    const k = kind.kind;
    if ("Song" in k) return songTitles[k.Song.song] ?? "Song";
    if ("Header" in k) return k.Header.title;
    if ("Scripture" in k) return "Scripture";
    if ("Deck" in k) return "Presentation";
    if ("Media" in k) return k.Media.name;
    if ("Countdown" in k) return k.Countdown.label;
    return "Announcements";
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

  refresh();
  loadDatabases();
  if (live.planning) select(live.planning);
</script>

<div class="grid">
  <Panel title="Services">
    <form
      class="row"
      onsubmit={(e) => {
        e.preventDefault();
        create();
      }}
    >
      <input aria-label="New service title" bind:value={newTitle} />
      <Button variant="primary" onclick={create}>New service</Button>
    </form>
    <ul class="list">
      {#each services as s (s.id)}
        <li>
          <span>{s.title}</span>
          <Button onclick={() => select(s.id)} pressed={selected?.id === s.id}>Edit</Button>
        </li>
      {:else}
        <li class="muted">No services yet.</li>
      {/each}
    </ul>
  </Panel>

  <Panel title={selected ? selected.title : "No service selected"}>
    {#snippet actions()}
      <Button variant="primary" onclick={openService} disabled={!selected}>Open for live</Button>
    {/snippet}
    {#if selected}
      <ul class="list">
        {#each ordered(selected) as [id, item], i (id)}
          <li>
            <span>{i + 1}. {itemTitle(item)}</span>
            <Button onclick={() => remove(id)}>Remove</Button>
          </li>
        {:else}
          <li class="muted">Empty. Add a song from the library.</li>
        {/each}
      </ul>
    {:else}
      <p class="muted">Create a service or pick one to edit.</p>
    {/if}
  </Panel>

  <Panel title="Songs">
    {#snippet actions()}
      <Button onclick={importFiles} disabled={busy}>Import files…</Button>
      <Button onclick={importFolder} disabled={busy}>Import folder…</Button>
    {/snippet}
    <details class="getsongs" open={hits.length <= 2}>
      <summary>Get songs</summary>
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
        <Button variant="primary" onclick={downloadDatabase} disabled={busy}>
          {busy ? "Working…" : "Download and import"}
        </Button>
      </div>
    </details>
    <form
      onsubmit={(e) => {
        e.preventDefault();
        search();
      }}
    >
      <input aria-label="Search songs" placeholder="Search titles and lyrics" bind:value={query} oninput={search} />
    </form>
    <ul class="list">
      {#each hits as hit (hit.song)}
        <li>
          <span>
            {hit.title}
            {#if hit.line}<br /><small class="muted">{hit.line}</small>{/if}
          </span>
          <Button onclick={() => addSong(hit.song)} disabled={!selected}>Add</Button>
        </li>
      {:else}
        <li class="muted">No songs match.</li>
      {/each}
    </ul>
  </Panel>
</div>

<style>
  .grid {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1.2fr) minmax(0, 1.2fr);
    gap: 1rem;
    align-items: start;
  }
  form input {
    width: 100%;
    min-width: 0;
    box-sizing: border-box;
    margin-bottom: 0.5rem;
  }
  .getsongs {
    margin-bottom: 0.8rem;
  }
  .getsongs summary {
    cursor: pointer;
    font-weight: 600;
  }
  .getsongs p {
    font-size: 0.85rem;
  }
  .row {
    display: flex;
    gap: 0.5rem;
    align-items: center;
  }
  form.row {
    display: flex;
    gap: 0.5rem;
    align-items: start;
  }
</style>
