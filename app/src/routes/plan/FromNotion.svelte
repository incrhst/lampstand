<!--
  Plan › New service › From Notion (T111, decision 0007). Paste the link
  to the week's service plan, check what Lampstand matched, and create the
  service. Nothing is written until Create. Esc closes.
-->

<script lang="ts">
  import { onMount } from "svelte";
  import {
    request,
    type Choice,
    type EntryChoice,
    type PlanDraft,
    type ResolvedEntry,
    type Service,
    type SongHit,
  } from "../../lib/api";
  import Button from "../../lib/kit/Button.svelte";
  import Icon from "../../lib/kit/Icon.svelte";
  import { formatPassage } from "../../lib/passage";
  import { deviceZone, offsetMinutes } from "../../lib/when";

  let {
    onclose,
    oncreated,
  }: {
    onclose: () => void;
    oncreated: (service: Service) => void;
  } = $props();

  let link = $state("");
  let reading = $state(false);
  let creating = $state(false);
  let error = $state("");
  let draft = $state<PlanDraft | null>(null);
  /** The person's choice per entry, by `source_id`. */
  let choices: Record<string, Choice> = $state({});
  /** The entry whose library search is open, and what it found. */
  let searching = $state<string | null>(null);
  let query = $state("");
  let hits: SongHit[] = $state([]);
  /** Titles of songs picked from a search, by song ID, for the row's label. */
  let picked: Record<string, string> = $state({});
  let input: HTMLInputElement | undefined = $state();

  onMount(() => {
    input?.focus();
    const previous = document.activeElement as HTMLElement | null;
    return () => previous?.focus?.({ preventScroll: true });
  });

  async function read() {
    if (!link.trim() || reading) return;
    reading = true;
    error = "";
    try {
      const zone = deviceZone();
      draft = await request(
        {
          area: "Plan",
          request: {
            DraftFromNotion: { link: link.trim(), time_zone: zone, utc_offset_minutes: offsetMinutes(Date.now(), zone) },
          },
        },
        "PlanDraft",
      );
      choices = {};
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      reading = false;
    }
  }

  /** Whether the person still has to decide this entry before Create. */
  function open(e: ResolvedEntry): boolean {
    const choice = choices[e.source_id];
    // A likely song counts as open until the person picks it.
    if (e.outcome.kind === "Song") return e.outcome.result.kind !== "Matched" && !choice;
    if (e.outcome.kind === "Scripture" && e.outcome.result.kind === "Choose") return !choice;
    return false;
  }

  let toCheck = $derived(draft ? draft.plan.entries.filter(open).length : 0);

  function choose(e: ResolvedEntry, choice: Choice) {
    choices[e.source_id] = choice;
    if (searching === e.source_id) searching = null;
  }

  /** What a song row's picker holds, as one value per option. */
  function songValue(e: ResolvedEntry): string {
    const c = choices[e.source_id];
    if (!c) return e.outcome.kind === "Song" && e.outcome.result.kind === "Matched" ? `song:${e.outcome.result.song}` : "";
    if (c.kind === "Song") return `song:${c.song}`;
    if (c.kind === "Accept" && e.outcome.kind === "Song" && e.outcome.result.kind === "Likely") return `song:${e.outcome.result.song}`;
    return c.kind;
  }

  function onSongPick(e: ResolvedEntry, value: string) {
    if (value === "search") {
      searching = e.source_id;
      // Search wants every word, so the bracketed part of a title goes.
      query = (e.outcome.kind === "Song" ? e.outcome.hint_title : e.title).replace(/\s*[([][^)\]]*[)\]]/g, "").trim();
      search();
      return;
    }
    if (value.startsWith("song:")) choose(e, { kind: "Song", song: value.slice(5) });
    else if (value === "CreateBlank") choose(e, { kind: "CreateBlank" });
    else if (value === "Placeholder") choose(e, { kind: "Placeholder" });
    else delete choices[e.source_id];
  }

  async function search() {
    const q = query.trim();
    if (!q) {
      hits = [];
      return;
    }
    try {
      hits = await request({ area: "Library", request: { SearchSongs: { query: q, limit: 8 } } }, "Songs");
    } catch {
      hits = [];
    }
  }

  function pickHit(e: ResolvedEntry, hit: SongHit) {
    picked[hit.song] = hit.title;
    choose(e, { kind: "Song", song: hit.song });
  }

  async function create() {
    if (!draft || toCheck > 0 || creating) return;
    creating = true;
    error = "";
    try {
      const list: EntryChoice[] = Object.entries(choices).map(([source_id, choice]) => ({ source_id, choice }));
      const service = await request({ area: "Plan", request: { CreateFromPlan: { draft: draft.plan, choices: list } } }, "Service");
      oncreated(service);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      creating = false;
    }
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      if (searching) searching = null;
      else onclose();
    }
  }

  const how = { Ccli: "by CCLI number", Remembered: "as picked before", Title: "by title" } as const;
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="backdrop" onclick={onclose}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="from-notion-title" tabindex="-1" {onkeydown} onclick={(e) => e.stopPropagation()}>
    <header>
      <h2 id="from-notion-title">New service from Notion</h2>
      <Button variant="ghost" size="small" icon="x" onclick={onclose}>Close</Button>
    </header>

    <form
      class="link"
      onsubmit={(e) => {
        e.preventDefault();
        read();
      }}
    >
      <label for="notion-link">Link to the service plan in Notion</label>
      <div class="row">
        <input id="notion-link" bind:this={input} bind:value={link} placeholder="https://www.notion.so/…" autocomplete="off" spellcheck="false" />
        <Button variant={draft ? "secondary" : "primary"} size="small" icon="download" onclick={read} disabled={reading || !link.trim()}>
          {reading ? "Reading…" : draft ? "Read again" : "Read plan"}
        </Button>
      </div>
      <p class="faint hint">The page must be shared to the web in Notion (Share › Publish). Nothing is added until you press Create.</p>
    </form>

    {#if error}
      <p class="error" role="alert"><Icon name="alert" size={14} />{error}</p>
    {/if}

    {#if draft}
      <section class="review" aria-label="What Lampstand found">
        <div class="summary">
          <strong>{draft.plan.title}</strong>
          {#if draft.plan.status === "draft"}<span class="tag">Draft in Notion</span>{/if}
        </div>
        {#if draft.existing}
          <p class="warn"><Icon name="info" size={14} />This plan was already imported as “{draft.existing.title}”. Delete that service to import it again.</p>
        {/if}
        {#each draft.warnings as w (w)}
          <p class="warn"><Icon name="alert" size={14} />{w}</p>
        {/each}

        <ol class="plain entries">
          {#each draft.plan.entries as e (e.source_id)}
            {@const o = e.outcome}
            <li class="entry" class:needs={open(e)} class:section={o.kind === "Header" && e.entry_type === "header"}>
              <span class="kind"><Icon name={o.kind === "Song" ? "song" : o.kind === "Scripture" ? "scripture" : e.entry_type === "slides" ? "deck" : e.entry_type === "announcement" ? "announcements" : "header"} size={14} /></span>
              <div class="what">
                <span class="title">{e.title}</span>
                {#if o.kind === "Header"}
                  {#if e.entry_type === "slides" || e.entry_type === "announcement"}<span class="muted">Deck to attach in Plan</span>{/if}
                {:else if o.kind === "Placeholder"}
                  <span class="muted">{o.reason}</span>
                {:else if o.kind === "Song"}
                  {@const r = o.result}
                  <span class="status">
                    {#if r.kind !== "Matched" && choices[e.source_id]}<span class="ok"><Icon name="check" size={12} />{choices[e.source_id]?.kind === "CreateBlank" ? "A new song will be added" : choices[e.source_id]?.kind === "Placeholder" ? "A placeholder will be left" : "Picked by you"}</span>
                    {:else if r.kind === "Matched"}<span class="ok"><Icon name="check" size={12} />In the library {how[r.how]}</span>
                    {:else if r.kind === "Likely"}<span class="check">Check: probably “{r.title}”</span>
                    {:else if r.kind === "Choose"}<span class="check">Pick one of {r.candidates.length}</span>
                    {:else}<span class="check">Not in the library</span>{/if}
                  </span>
                  <select aria-label="Song for {e.title}" value={songValue(e)} onchange={(ev) => onSongPick(e, (ev.currentTarget as HTMLSelectElement).value)}>
                    {#if r.kind !== "Matched"}<option value="" disabled>Choose…</option>{/if}
                    {#if r.kind === "Matched" || r.kind === "Likely"}<option value="song:{r.song}">Use “{r.title}”</option>{/if}
                    {#if r.kind === "Choose"}
                      {#each r.candidates as c (c.song)}<option value="song:{c.song}">Use “{c.title}”{c.author ? ` · ${c.author}` : ""}</option>{/each}
                    {/if}
                    {#each Object.entries(picked) as [id, title] (id)}
                      {#if choices[e.source_id]?.kind === "Song" && (choices[e.source_id] as { song: string }).song === id}<option value="song:{id}">Use “{title}”</option>{/if}
                    {/each}
                    <option value="search">Search the library…</option>
                    <option value="CreateBlank">Add “{o.hint_title}” as a new song (no words yet)</option>
                    <option value="Placeholder">Leave a placeholder</option>
                  </select>
                  {#if searching === e.source_id}
                    <div class="search">
                      <input aria-label="Search songs for {e.title}" bind:value={query} oninput={search} />
                      <ul class="plain hits">
                        {#each hits as h (h.song)}
                          <li><button type="button" onclick={() => pickHit(e, h)}>{h.title}</button></li>
                        {:else}
                          <li class="muted">No songs found.</li>
                        {/each}
                      </ul>
                    </div>
                  {/if}
                {:else if o.kind === "Scripture"}
                  {@const r = o.result}
                  {#if r.kind === "Found"}
                    <span class="status"><span class="ok"><Icon name="check" size={12} />{formatPassage(r.passage)} · {r.translation_name}</span></span>
                  {:else}
                    <select aria-label="Passage for {e.title}" value={choices[e.source_id]?.kind === "Passage" ? String((choices[e.source_id] as { index: number }).index) : ""} onchange={(ev) => { const v = (ev.currentTarget as HTMLSelectElement).value; choose(e, v === "Placeholder" ? { kind: "Placeholder" } : { kind: "Passage", index: Number(v) }); }}>
                      <option value="" disabled>Which book?</option>
                      {#each r.candidates as p, i (i)}<option value={String(i)}>{formatPassage(p)} · {r.translation_name}</option>{/each}
                      <option value="Placeholder">Leave a placeholder</option>
                    </select>
                  {/if}
                {/if}
                {#if e.notes && o.kind !== "Placeholder"}
                  <span class="notes faint">{e.notes.split("\n").filter((l) => !l.startsWith("Planned for")).join(" · ")}</span>
                {/if}
                {#each e.warnings as w (w)}<span class="warn small"><Icon name="alert" size={12} />{w}</span>{/each}
              </div>
            </li>
          {/each}
        </ol>
      </section>

      <footer>
        <span id="from-notion-left" class="muted" aria-live="polite">
          {toCheck === 0 ? `${draft.plan.entries.length} items ready` : `${toCheck} ${toCheck === 1 ? "item" : "items"} still to check`}
        </span>
        <Button variant="ghost" size="small" onclick={onclose}>Cancel</Button>
        <Button variant="primary" size="small" icon="plus" onclick={create} disabled={toCheck > 0 || creating || !!draft.existing} describedby="from-notion-left">
          {creating ? "Creating…" : "Create service"}
        </Button>
      </footer>
    {/if}
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
    padding-top: 6vh;
    background: rgb(0 0 0 / 0.5);
  }
  .dialog {
    display: flex;
    flex-direction: column;
    width: min(44rem, calc(100% - 2rem));
    max-height: 88vh;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    box-shadow: 0 1rem 3rem rgb(0 0 0 / 0.5);
    overflow: hidden;
  }
  header,
  footer {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.75rem 1rem;
  }
  header {
    justify-content: space-between;
    border-bottom: 1px solid var(--line);
  }
  h2 {
    margin: 0;
    font-size: 1rem;
  }
  footer {
    justify-content: flex-end;
    border-top: 1px solid var(--line);
  }
  footer > span {
    margin-right: auto;
    font-size: 0.875rem;
  }
  .link {
    display: grid;
    gap: 0.35rem;
    padding: 0.9rem 1rem 0.5rem;
  }
  .link label {
    font-size: 0.875rem;
    font-weight: 500;
  }
  .row {
    display: flex;
    gap: 0.5rem;
  }
  .row input {
    flex: 1;
    min-width: 0;
  }
  .hint {
    margin: 0;
    font-size: 0.8125rem;
  }
  .error,
  .warn {
    display: flex;
    align-items: baseline;
    gap: 0.35rem;
    margin: 0.25rem 1rem;
    font-size: 0.875rem;
  }
  .error {
    color: var(--danger, #e5484d);
  }
  .warn {
    color: var(--muted);
  }
  .warn.small {
    margin: 0;
    font-size: 0.8125rem;
  }
  .review {
    overflow-y: auto;
    padding-bottom: 0.5rem;
  }
  .summary {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.5rem 1rem;
  }
  .tag {
    padding: 0 0.4rem;
    font-size: 0.75rem;
    color: var(--muted);
    border: 1px solid var(--line);
    border-radius: 999px;
  }
  .entries {
    margin: 0;
    padding: 0 0.5rem;
  }
  .entry {
    display: flex;
    gap: 0.6rem;
    padding: 0.45rem 0.5rem;
    border-left: 3px solid transparent;
    border-radius: var(--radius);
  }
  .entry.needs {
    border-left-color: var(--gold, #d4a72c);
    background: rgb(212 167 44 / 0.06);
  }
  .entry.section .title {
    font-weight: 600;
  }
  .kind {
    padding-top: 0.15rem;
    color: var(--muted);
  }
  .what {
    display: grid;
    gap: 0.25rem;
    min-width: 0;
    flex: 1;
  }
  .title {
    font-weight: 500;
  }
  .status {
    font-size: 0.8125rem;
  }
  .ok {
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
    color: var(--ok, #3fb950);
  }
  .check {
    color: var(--gold, #d4a72c);
  }
  .notes {
    font-size: 0.8125rem;
  }
  select {
    max-width: 100%;
    justify-self: start;
  }
  .search {
    display: grid;
    gap: 0.25rem;
  }
  .hits button {
    width: 100%;
    padding: 0.3rem 0.5rem;
    font: inherit;
    text-align: left;
    color: var(--text);
    background: transparent;
    border: 0;
    border-radius: var(--radius);
    cursor: pointer;
  }
  .hits button:hover,
  .hits button:focus-visible {
    background: var(--hover, rgb(255 255 255 / 0.07));
  }
</style>
