<!-- Present › Sound: what plays, who plays it, and the sound the service has.
     Shown only where there is sound to play (a church that does not use sound
     sees nothing). It works from any device: the request goes to whichever
     device runs the sound. The panic stop is always there and always works. -->
<script lang="ts">
  import { onMount } from "svelte";
  import type { LoadedItemView, Service } from "../../lib/api";
  import { live, refreshSound, report, sound } from "../../lib/state.svelte";
  import {
    isPlayingEntry,
    notice,
    progressFraction,
    progressLabel,
    showPanel,
    soundEntries,
    transport,
    whereLine,
    words,
    type SoundEntry,
  } from "../../lib/audio";
  import Banner from "../../lib/kit/Banner.svelte";
  import Button from "../../lib/kit/Button.svelte";
  import Icon from "../../lib/kit/Icon.svelte";
  import StatusPill from "../../lib/kit/StatusPill.svelte";

  let { items, service }: { items: LoadedItemView[]; service: Service | null } = $props();

  let entries = $derived(soundEntries(items, service));
  let view = $derived(live.sound);
  let visible = $derived(showPanel(entries, view));
  let playing = $derived(view?.playing ?? null);
  let can = $derived(transport(playing));
  let warning = $derived(notice(view));
  let where = $derived(whereLine(view));

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
</script>

{#if visible}
  <section class="sound" aria-label={words.title}>
    <div class="head">
      <h2><Icon name="sound" size={14} />{words.title}</h2>
      {#if where}<span class="muted where">{where}</span>{/if}
    </div>

    {#if refused || warning}
      <Banner tone="warn">{refused ?? warning?.text}</Banner>
    {/if}

    <div class="now" aria-live="polite">
      {#if playing}
        <div class="line">
          <StatusPill tone={playing.paused ? "neutral" : "live"}>{playing.paused ? words.paused : playing.ending ? words.fading : "PLAYING"}</StatusPill>
          <span class="name">{playing.name}</span>
        </div>
        <div class="bar" role="progressbar" aria-label={playing.name} aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.round(progressFraction(playing, live.soundAt, now) * 100)}>
          <span style:width="{progressFraction(playing, live.soundAt, now) * 100}%"></span>
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

    {#if entries.length > 0}
      <h3>{words.inService}</h3>
      <ul class="plain entries">
        {#each entries as entry (entry.item + (entry.backing ? "-track" : ""))}
          <li class:current={isPlayingEntry(entry, playing)}>
            <div class="text">
              <span class="name">{entry.name}</span>
              <span class="muted small">{entry.detail}</span>
            </div>
            <Button size="small" icon="play" label="{words.play} {entry.name}" onclick={() => playEntry(entry)}>{words.play}</Button>
          </li>
        {/each}
      </ul>
    {/if}
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
  .head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.5rem;
  }
  h2 {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.85rem;
    font-weight: 600;
    margin: 0;
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
  .text {
    display: grid;
    min-width: 0;
  }
</style>
