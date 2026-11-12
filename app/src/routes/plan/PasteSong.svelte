<!-- Paste lyrics: the words of a song from a web page, an email or a chart,
     read into sections and checked here before they are added. Nothing is
     saved until the person presses a button. The reading is the core's
     (`ParseLyrics`), so the preview is exactly what will be saved. -->
<script lang="ts">
  import { onMount } from "svelte";
  import { request, send, type ItemKind, type ParsedSong, type Service } from "../../lib/api";
  import { live, report } from "../../lib/state.svelte";
  import {
    blankDetails,
    effectiveTitle,
    problem,
    previewRows,
    sungOrder,
    titleFromText,
    toSong,
    words,
  } from "../../lib/pasteSong";
  import Banner from "../../lib/kit/Banner.svelte";
  import Button from "../../lib/kit/Button.svelte";

  let {
    service,
    onadd,
    onclose,
  }: {
    /** The service the song can also be added to, if one is open in Plan. */
    service: Service | null;
    /** Adds an item at the end of that service; resolves true once it is in. */
    onadd: (kind: ItemKind) => Promise<boolean>;
    onclose: () => void;
  } = $props();

  let text = $state("");
  let details = $state(blankDetails());
  let parsed = $state<ParsedSong | null>(null);
  let readError = $state<string | null>(null);
  let saving = $state(false);
  let failed = $state("");
  let box: HTMLTextAreaElement | undefined = $state();

  onMount(() => box?.focus());

  // Read what is pasted a moment after the typing stops. An answer that
  // arrives after a newer question was sent is dropped.
  let asked = 0;
  $effect(() => {
    const body = text;
    const title = details.title;
    const mine = ++asked;
    if (body.trim() === "") {
      parsed = null;
      readError = null;
      return;
    }
    const timer = setTimeout(() => {
      request({ area: "Library", request: { ParseLyrics: { text: body, title: title.trim() === "" ? null : title.trim() } } }, "ParsedSong")
        .then((p) => {
          if (mine !== asked) return;
          parsed = p;
          readError = null;
        })
        .catch((e) => {
          if (mine !== asked) return;
          parsed = null;
          readError = e instanceof Error ? e.message : String(e);
        });
    }, 250);
    return () => clearTimeout(timer);
  });

  let rows = $derived(previewRows(parsed));
  let order = $derived(sungOrder(parsed));
  let name = $derived(effectiveTitle(details.title, parsed));
  let fromText = $derived(titleFromText(details.title, parsed));
  let blocked = $derived(problem(details, parsed, readError));
  let ready = $derived(parsed !== null && blocked === null && !saving);

  async function add(toService: boolean) {
    if (!parsed || blocked !== null || saving) return;
    saving = true;
    failed = "";
    try {
      const song = toSong(parsed, details);
      await send({ area: "Library", request: { PutSong: song } });
      live.songsVersion++;
      if (toService && service) {
        const put = await onadd({ Song: { song: song.id, arrangement: null } });
        live.status = put ? words.addedToService(song.title, service.title) : words.added(song.title);
      } else {
        live.status = words.added(song.title);
      }
      onclose();
    } catch (e) {
      failed = e instanceof Error ? e.message : String(e);
      report(e);
    } finally {
      saving = false;
    }
  }

  function onkey(e: KeyboardEvent) {
    if (e.key === "Escape") onclose();
  }
</script>

<svelte:window onkeydown={onkey} />

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="backdrop" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog" role="dialog" aria-modal="true" aria-label={words.title}>
    <header>
      <h2>{words.title}</h2>
      <Button variant="ghost" size="small" icon="x" label="Close" onclick={onclose} />
    </header>
    <div class="body">
      <div class="columns">
        <div class="left">
          <label>
            {words.pasteLabel}
            <textarea
              bind:this={box}
              rows="14"
              spellcheck="false"
              bind:value={text}
              aria-describedby="paste-hint"
            ></textarea>
          </label>
          <p id="paste-hint" class="muted small">{words.pasteHint}</p>
          <label>
            {words.titleLabel}
            <input type="text" bind:value={details.title} />
          </label>
          {#if fromText}<p class="muted small" role="status">{words.titleFromText(fromText)}</p>{/if}
          <details>
            <summary>{words.moreDetails}</summary>
            <div class="fields">
              <label>{words.authorLabel}<input type="text" bind:value={details.author} /></label>
              <label>{words.copyrightLabel}<input type="text" bind:value={details.copyright} /></label>
              <label>{words.ccliLabel}<input type="text" inputmode="numeric" bind:value={details.ccli} /></label>
              <label>{words.keyLabel}<input type="text" bind:value={details.key} placeholder="G" /></label>
            </div>
          </details>
        </div>
        <div class="right" aria-live="polite">
          <h3>{words.previewTitle}</h3>
          {#if rows.length === 0}
            <p class="muted small">{readError ?? words.empty}</p>
          {:else}
            <ol class="preview">
              {#each rows as row}
                <li><b>{row.label}</b><span class="muted"> · {row.detail}</span><span class="first">{row.first}</span></li>
              {/each}
            </ol>
            {#if order}<p class="muted small">{words.sung(order)}</p>{/if}
          {/if}
          {#if parsed}
            {#each parsed.warnings as warning}
              <Banner tone="neutral">{warning}</Banner>
            {/each}
            {#if parsed.same_title}
              <Banner tone="warn">{words.sameTitle(name)}</Banner>
            {/if}
          {/if}
        </div>
      </div>
    </div>
    <footer>
      <span class="problems" role="status">{failed || (parsed ? blocked : null) || ""}</span>
      <span class="muted small licence">{words.licence}</span>
      <Button variant="ghost" onclick={onclose}>{words.cancel}</Button>
      <Button variant={service ? "secondary" : "primary"} onclick={() => add(false)} disabled={!ready}>{words.addToLibrary}</Button>
      {#if service}
        <Button variant="primary" onclick={() => add(true)} disabled={!ready}>{words.addToService(service.title)}</Button>
      {/if}
    </footer>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.55);
  }
  .dialog {
    width: min(60rem, calc(100vw - 2rem));
    max-height: calc(100vh - 3rem);
    display: flex;
    flex-direction: column;
    background: var(--panel);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-lg);
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
  footer {
    border-top: 1px solid var(--line);
    flex-wrap: wrap;
  }
  h2,
  h3 {
    margin: 0;
    font-size: 1rem;
  }
  h3 {
    font-size: 0.875rem;
    margin-bottom: 0.5rem;
  }
  .body {
    overflow-y: auto;
    padding: 1rem;
  }
  .columns {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 1.25rem;
  }
  @media (max-width: 720px) {
    .columns {
      grid-template-columns: 1fr;
    }
  }
  .left,
  .right {
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
    min-width: 0;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    font-size: 0.75rem;
    color: var(--muted);
  }
  .fields {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.6rem;
    margin-top: 0.6rem;
  }
  summary {
    cursor: pointer;
    font-size: 0.8rem;
    color: var(--muted);
  }
  input,
  textarea {
    font: inherit;
    font-size: 0.875rem;
    color: var(--text);
    background: var(--canvas);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    padding: 0.4rem 0.6rem;
  }
  textarea {
    width: 100%;
    box-sizing: border-box;
    resize: vertical;
    line-height: 1.4;
  }
  textarea:focus,
  input:focus {
    outline: none;
    border-color: var(--gold);
  }
  .small {
    font-size: 0.8rem;
    margin: 0;
  }
  .preview {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
  }
  .preview li {
    display: flex;
    flex-wrap: wrap;
    gap: 0.3rem;
    font-size: 0.875rem;
  }
  .preview .first {
    flex-basis: 100%;
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: 0.8rem;
  }
  .problems {
    flex: 1;
    color: var(--warn);
    font-size: 0.8rem;
    min-width: 8rem;
  }
  .licence {
    flex-basis: 100%;
    order: 5;
  }
</style>
