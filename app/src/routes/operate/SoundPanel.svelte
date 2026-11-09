<!-- Present › Sound, in the press block. At rest it shows nothing: music
     plays from Play on its item on the wall. While something plays: its
     name, how far it has got, Pause or Resume, Fade out, and Stop the sound
     now in solid red. A sound the service has reached that waits for Play (a
     backing track) shows with its own Play. A problem with the sound shows
     here too, in words. It works from any device: the request goes to
     whichever device runs the sound. With the modifier chord held, Stop
     shows M and the waiting Play shows P. -->
<script lang="ts">
  import { onMount, untrack } from "svelte";
  import type { LoadedItemView, Service } from "../../lib/api";
  import { live, refreshSound, report, sound } from "../../lib/state.svelte";
  import {
    hear,
    notice,
    progressFraction,
    progressLabel,
    soundEntries,
    soundQueue,
    transport,
    whereLine,
    words,
    type Heard,
    type SoundEntry,
  } from "../../lib/audio";
  import Icon from "../../lib/kit/Icon.svelte";

  let {
    items,
    service,
    chord = false,
    waiting = $bindable(null),
  }: { items: LoadedItemView[]; service: Service | null; chord?: boolean; waiting?: SoundEntry | null } = $props();

  let entries = $derived(soundEntries(items, service));
  let view = $derived(live.sound);
  let playing = $derived(view?.playing ?? null);
  let can = $derived(transport(playing));
  let warning = $derived(notice(view));
  let where = $derived(whereLine(view));

  let liveItem = $derived(live.live?.item ?? null);
  /** Sound that has played since its item went live, so a waiting one stops asking. */
  let heard: Heard = $state({ at: null, keys: new Set() });
  $effect(() => {
    const next = hear(untrack(() => heard), liveItem, playing, entries);
    untrack(() => {
      if (next !== heard) heard = next;
    });
  });
  let queue = $derived(soundQueue(entries, items.map((i) => i.id), liveItem, heard.keys));
  // Present's modifier chord plays the waiting sound with P.
  $effect(() => {
    const next = queue.waiting ?? null;
    untrack(() => {
      if (next !== waiting) waiting = next;
    });
  });

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

  /** An action's own error is the core's sentence; it shows in the row, not only the status bar. */
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

{#if playing || queue.waiting || message}
<div class="sound-row" class:playing={!!playing}>
  <p class="line" aria-live="polite">
    {#if playing}
      <span class="what">{playing.paused ? words.paused : playing.ending ? words.fading : words.playing}: <strong>{playing.name}</strong></span>
      <span class="time">{progressLabel(playing, live.soundAt, now)}</span>
    {:else}
      <span class="what">{where ?? "No sound playing"}</span>
    {/if}
  </p>
  {#if playing}
    <div class="bar" role="progressbar" aria-label={playing.name} aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.round(fraction * 100)}>
      <span style:width="{fraction * 100}%"></span>
    </div>
  {/if}

  {#if queue.waiting}
    <p class="waiting">
      <span class="what">{words.waiting}: <strong>{queue.waiting.name}</strong></span>
      <button type="button" class="btn play" onclick={() => queue.waiting && playEntry(queue.waiting)}>{#if chord}<span class="chord-key" aria-hidden="true">P</span>{/if}<Icon name="play" size={12} />{words.play}</button>
    </p>
  {/if}

  {#if playing}
  <div class="buttons">
      {#if can.resume}
        <button type="button" class="btn" onclick={() => act(() => sound("Resume"))}>{words.resume}</button>
      {:else}
        <button type="button" class="btn" disabled={!can.pause} onclick={() => act(() => sound("Pause"))}>{words.pause}</button>
      {/if}
      <button type="button" class="btn" disabled={!can.fadeOut} onclick={() => act(() => sound("Stop"))}>{words.fadeOut}</button>
    <span class="gap"></span>
    <button type="button" class="btn stop" onclick={() => act(() => sound("Panic"))} title="Stops every sound at once">{#if chord}<span class="chord-key" aria-hidden="true">M</span>{/if}{words.panic}</button>
  </div>
  {/if}

  <!-- Always in the page, so a message that arrives is announced once. -->
  <div role="status">
    {#if message}
      <p class="problem"><Icon name="alert" size={14} /><span>{message}</span></p>
    {/if}
  </div>
</div>
{/if}

<style>
  .sound-row {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    font-size: 0.875rem;
  }
  .line,
  .waiting {
    margin: 0;
    display: flex;
    align-items: baseline;
    gap: 0.5rem;
    min-width: 0;
  }
  .what {
    flex: 1;
    min-width: 0;
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .what strong {
    color: var(--text);
    font-weight: 600;
  }
  .time {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
    font-size: 0.8125rem;
  }
  .bar {
    height: 3px;
    border-radius: 2px;
    background: var(--line);
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--gold);
  }
  .waiting {
    align-items: center;
  }
  .buttons {
    display: flex;
    align-items: center;
    gap: 0.35rem;
  }
  .gap {
    flex: 1;
  }
  .btn {
    position: relative;
    font: inherit;
    font-size: 0.8125rem;
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0.3rem 0.65rem;
    color: var(--text);
    background: transparent;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius);
    cursor: pointer;
  }
  .btn:hover:not(:disabled) {
    border-color: var(--gold-hover);
  }
  .btn:disabled {
    opacity: 0.45;
    cursor: default;
  }
  .btn.play {
    border-color: var(--gold);
    font-weight: 600;
  }
  /* Stop the sound now: always here. Outlined at rest, solid while sound plays. */
  .btn.stop {
    color: var(--live-text, var(--live));
    border-color: var(--live);
    font-weight: 600;
  }
  .playing .btn.stop {
    color: #fff;
    background: var(--live);
  }
  .chord-key {
    position: absolute;
    top: -0.45rem;
    left: -0.35rem;
    min-width: 1.35rem;
    padding: 0.05rem 0.3rem;
    border-radius: 5px;
    background: var(--gold);
    color: var(--canvas);
    font: 700 0.72rem/1.2 ui-monospace, Menlo, monospace;
    text-align: center;
    box-shadow: 0 2px 6px rgba(0, 0, 0, 0.5);
    pointer-events: none;
  }
  .problem {
    margin: 0;
    display: flex;
    gap: 0.4rem;
    align-items: flex-start;
    padding: 0.45rem 0.6rem;
    border-radius: var(--radius);
    background: var(--warn-soft, rgba(245, 165, 36, 0.12));
    box-shadow: inset 0 0 0 1px var(--warn, #f5a524);
    font-size: 0.8125rem;
  }
</style>
