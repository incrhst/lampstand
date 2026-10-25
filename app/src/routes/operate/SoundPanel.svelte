<!-- Present › Sound: what plays, who plays it, and the sound the service has.
     Shown only where there is sound to play (a church that does not use sound
     sees nothing). It works from any device: the request goes to whichever
     device runs the sound. The panic stop is always there and always works.

     The box folds to one line (#191): what plays, Fade out and the panic
     stop; a warning and a sound that waits for Play still show. It is open
     before the service starts and folded once it is under way, unless chosen
     otherwise on this computer (lib/disclosure.ts). The service's sound is a
     queue that follows the service: "Up next" by default, "Show all" for the
     rest, with sound already played greyed and still playable. -->
<script lang="ts">
  import { onMount, untrack } from "svelte";
  import type { LoadedItemView, Service } from "../../lib/api";
  import { live, refreshSound, report, sound } from "../../lib/state.svelte";
  import {
    entryKey,
    entryProblem,
    foldedLine,
    hear,
    isPlayingEntry,
    leftLabel,
    notice,
    noticeOnEntry,
    progressFraction,
    progressLabel,
    showPanel,
    soundEntries,
    soundQueue,
    transport,
    whereLine,
    words,
    type Heard,
    type SoundEntry,
  } from "../../lib/audio";
  import { soundKey, soundOpen } from "../../lib/disclosure";
  import Button from "../../lib/kit/Button.svelte";
  import Disclosure from "../../lib/kit/Disclosure.svelte";
  import Icon from "../../lib/kit/Icon.svelte";
  import StatusPill from "../../lib/kit/StatusPill.svelte";

  let { items, service }: { items: LoadedItemView[]; service: Service | null } = $props();

  let entries = $derived(soundEntries(items, service));
  let view = $derived(live.sound);
  let visible = $derived(showPanel(entries, view));
  let playing = $derived(view?.playing ?? null);
  let can = $derived(transport(playing));
  let warning = $derived(notice(view));
  /** The message is already shown next to its item: the panel's copy is then for a screen reader only. */
  let onItem = $derived(noticeOnEntry(view, entries));
  let where = $derived(whereLine(view));

  let liveItem = $derived(live.live?.item ?? null);
  let underWay = $derived(liveItem != null);
  /** Sound that has played since its item went live, so a waiting one stops asking. */
  let heard: Heard = $state({ at: null, keys: new Set() });
  $effect(() => {
    const next = hear(untrack(() => heard), liveItem, playing, entries);
    untrack(() => {
      if (next !== heard) heard = next;
    });
  });
  let queue = $derived(soundQueue(entries, items.map((i) => i.id), liveItem, heard.keys));

  let open = $state(untrack(() => soundOpen(underWay)));
  let phase = untrack(() => underWay);
  // The service starting (or ending) brings that time's choice back.
  $effect(() => {
    const now = underWay;
    untrack(() => {
      if (now !== phase) {
        phase = now;
        open = soundOpen(now);
      }
    });
  });
  /** "Show all": the whole list instead of Up next. Not remembered. */
  let all = $state(false);
  const id = $props.id();

  // The position moves on by itself between what the core says.
  let now = $state(Date.now());
  $effect(() => {
    if (!playing || playing.paused) return;
    const timer = setInterval(() => (now = Date.now()), 500);
    return () => clearInterval(timer);
  });

  onMount(() => {
    refreshSound();
  });

  /** An action's own error is the core's sentence; it shows in the panel's banner, not only the status bar. */
  let refused: string | null = $state(null);
  let message = $derived(refused ?? warning?.text ?? null);
  async function act(run: () => Promise<unknown>) {
    refused = null;
    try {
      await run();
    } catch (e) {
      refused = e instanceof Error ? e.message : String(e);
      report(e);
    }
  }

  const playEntry = (entry: SoundEntry) => act(() => sound({ Play: { item: entry.item, backing: entry.backing } }));
  const fraction = $derived(playing ? progressFraction(playing, live.soundAt, now) : 0);
</script>

{#snippet row(entry: SoundEntry, kind: "waiting" | "played" | "")}
  <li class:current={isPlayingEntry(entry, playing)} class:waiting={kind === "waiting"} class:played={kind === "played"}>
    <div class="text">
      <span class="name">{entry.name}</span>
      <span class="muted small">{kind === "waiting" ? `${words.waiting} · ${entry.detail}` : entry.detail}</span>
    </div>
    <Button size="small" icon="play" variant={kind === "waiting" ? "primary" : "secondary"} label="{words.play} {entry.name}" onclick={() => playEntry(entry)}>{words.play}</Button>
    {#if entryProblem(view, entry)}
      <!-- The same sentence as the panel's message, which a screen reader hears there. -->
      <p class="problem" aria-hidden="true">
        <span class="icon"><Icon name="alert" size={14} /></span>
        <span>{entryProblem(view, entry)}</span>
      </p>
    {/if}
  </li>
{/snippet}

{#if visible}
  <section class="sound" class:folded={!open} aria-label={words.title}>
    <div class="head">
      <h2>
        <Disclosure bind:open label={words.title} controls={id} key={soundKey(underWay)}>
          <Icon name="sound" size={14} />{words.title}
        </Disclosure>
      </h2>
      {#if open}
        {#if where}<span class="muted where">{where}</span>{/if}
      {:else}
        <span class="summary">
          <span class="name">· {foldedLine(playing)}</span>
          {#if playing}
            <span class="bar mini" aria-hidden="true"><span style:width="{fraction * 100}%"></span></span>
            <span class="muted small">{leftLabel(playing, live.soundAt, now)}</span>
          {/if}
        </span>
        <span class="transport">
          {#if playing}<Button size="small" disabled={!can.fadeOut} onclick={() => act(() => sound("Stop"))}>{words.fadeOut}</Button>{/if}
          <Button size="small" variant="danger" onclick={() => act(() => sound("Panic"))}>{words.panic}</Button>
        </span>
      {/if}
    </div>

    <!-- Always in the page, so a message that arrives is announced once. -->
    <div class="notice" role="status">
      {#if message}
        <p class="message" class:sr-only={onItem && !refused}>
          <span class="icon" aria-hidden="true"><Icon name="alert" size={14} /></span>
          <span>{message}</span>
        </p>
      {/if}
    </div>

    <div class="body" {id}>
      {#if open}
        <div class="now" aria-live="polite">
          {#if playing}
            <div class="line">
              <StatusPill tone={playing.paused ? "neutral" : "live"}>{playing.paused ? words.paused : playing.ending ? words.fading : words.playing}</StatusPill>
              <span class="name">{playing.name}</span>
            </div>
            <div class="bar" role="progressbar" aria-label={playing.name} aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.round(fraction * 100)}>
              <span style:width="{fraction * 100}%"></span>
            </div>
            <span class="muted small">{progressLabel(playing, live.soundAt, now)}</span>
          {:else}
            <p class="muted small">{words.quiet}</p>
          {/if}
          <div class="transport">
            {#if can.resume}
              <Button size="small" icon="play" onclick={() => act(() => sound("Resume"))}>{words.resume}</Button>
            {:else}
              <Button size="small" disabled={!can.pause} onclick={() => act(() => sound("Pause"))}>{words.pause}</Button>
            {/if}
            <Button size="small" disabled={!can.fadeOut} onclick={() => act(() => sound("Stop"))}>{words.fadeOut}</Button>
            <Button size="small" variant="danger" onclick={() => act(() => sound("Panic"))}>{words.panic}</Button>
          </div>
        </div>
      {/if}

      <!-- The service has reached a sound that waits for Play: shown even when folded. -->
      {#if queue.waiting}
        <ul class="plain entries">{@render row(queue.waiting, "waiting")}</ul>
      {/if}

      {#if open && entries.length > 0}
        {#if all}
          {#if queue.played.length > 0}
            <h3>{words.played(queue.played.length)}</h3>
            <ul class="plain entries">
              {#each queue.played as entry (entryKey(entry))}{@render row(entry, "played")}{/each}
            </ul>
          {/if}
          {#if queue.rest.some((e) => e !== queue.waiting)}
            <h3>{words.inService}</h3>
            <ul class="plain entries">
              {#each queue.rest.filter((e) => e !== queue.waiting) as entry (entryKey(entry))}{@render row(entry, "")}{/each}
            </ul>
          {/if}
        {:else if queue.upNext}
          <h3>{words.upNext}</h3>
          <ul class="plain entries">{@render row(queue.upNext, "")}</ul>
        {/if}
        <button type="button" class="link" aria-expanded={all} onclick={() => (all = !all)}>{all ? words.showLess : words.showAll(entries.length)}</button>
      {/if}
    </div>
  </section>
{/if}

<style>
  .sound {
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    background: var(--raised);
    padding: 0.7rem 0.8rem;
    margin: 0 0 0.75rem;
    display: grid;
    gap: 0.6rem;
  }
  .sound.folded {
    padding: 0.45rem 0.6rem;
    gap: 0.4rem;
  }
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 0.4rem 0.5rem;
    min-width: 0;
  }
  .folded .head {
    justify-content: flex-start;
  }
  h2 {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.85rem;
    font-weight: 600;
    margin: 0;
  }
  h2 :global(.disclosure) {
    gap: 0.4rem;
  }
  h3 {
    font-size: 0.75rem;
    font-weight: 600;
    color: var(--muted);
    margin: 0.2rem 0 0;
  }
  .small {
    font-size: 0.8rem;
  }
  .where {
    font-size: 0.8rem;
  }
  .summary {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    min-width: 0;
    flex: 1 1 8rem;
    font-size: 0.85rem;
  }
  .body {
    display: grid;
    gap: 0.6rem;
  }
  .body:empty {
    display: none;
  }
  .now {
    display: grid;
    gap: 0.4rem;
  }
  .line {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    min-width: 0;
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .bar {
    height: 4px;
    border-radius: 2px;
    background: var(--line);
    overflow: hidden;
  }
  .bar.mini {
    flex: 0 0 3.5rem;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--accent);
  }
  .transport {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
  }
  .entries {
    display: grid;
    gap: 0.35rem;
  }
  .entries li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    padding: 0.3rem 0.4rem;
    border-radius: var(--radius);
  }
  .entries li.current {
    background: var(--panel);
    outline: 1px solid var(--accent);
  }
  .entries li.waiting {
    background: var(--warn-soft);
    outline: 1px solid color-mix(in srgb, var(--warn) 50%, transparent);
  }
  .entries li.played .text {
    opacity: 0.6;
  }
  .text {
    display: grid;
    min-width: 0;
  }
  .link {
    justify-self: start;
    font: inherit;
    font-size: 0.8rem;
    background: none;
    border: none;
    padding: 0;
    color: var(--text);
    text-decoration: underline;
    cursor: pointer;
  }
  .message,
  .problem {
    display: flex;
    align-items: flex-start;
    gap: 0.5rem;
    margin: 0;
    font-size: 0.85rem;
  }
  .message {
    border: 1px solid color-mix(in srgb, var(--warn) 30%, transparent);
    border-radius: var(--radius-lg);
    background: var(--warn-soft);
    padding: 0.5rem 0.75rem;
  }
  .problem {
    flex-basis: 100%;
  }
  .icon {
    color: var(--warn);
    margin-top: 0.1rem;
  }
  .notice:empty {
    display: none;
  }
</style>
