<!-- Lampstand Remote's Sound box, as in Present: what plays, Pause or
     Resume and Fade out, the sound that waits for Play, Up next, and the
     whole list on asking. It folds out above the bottom bar on a phone and
     below the top bar on a tablet. "Stop the sound now" is not in here: it
     stays in the bar, always in reach. -->
<script lang="ts">
  import type { SoundRequest } from "../lib/api";
  import { untrack } from "svelte";
  import { words as soundWords, entryKey, hear, notice, progressFraction, progressLabel, soundQueue, transport, whereLine, type Heard } from "../lib/audio.ts";
  import Icon from "../lib/kit/Icon.svelte";
  import StatusPill from "../lib/kit/StatusPill.svelte";
  import { remoteSounds, type RemoteSound } from "../lib/remote.ts";
  import { link } from "./link.svelte";

  let { onsound }: { onsound: (request: SoundRequest) => void } = $props();

  let sounds = $derived(remoteSounds(link.loaded));
  let playing = $derived(link.sound?.playing ?? null);
  let can = $derived(transport(playing));
  let warning = $derived(notice(link.sound));
  let where = $derived(whereLine(link.sound));
  let liveItem = $derived(link.live?.live?.item ?? null);
  let heard: Heard = $state({ at: null, keys: new Set() });
  $effect(() => {
    const next = hear(untrack(() => heard), liveItem, playing, sounds);
    untrack(() => {
      if (next !== heard) heard = next;
    });
  });
  let queue = $derived(soundQueue(sounds, link.loaded?.items.map((i) => i.id) ?? [], liveItem, heard.keys));
  /** "Show all": the whole list instead of Up next. */
  let all = $state(false);

  // The position moves on by itself between what the laptop says.
  let now = $state(Date.now());
  $effect(() => {
    if (!playing || playing.paused) return;
    const timer = setInterval(() => (now = Date.now()), 500);
    return () => clearInterval(timer);
  });
</script>

<section class="sound" aria-label={soundWords.title}>
  <div class="head">
    <h2><Icon name="sound" size={16} />{soundWords.title}</h2>
    {#if where}<span class="muted where">{where}</span>{/if}
  </div>
  {#if warning}<p class="warning" role="status"><Icon name="alert" size={16} />{warning.text}</p>{/if}
  <div class="playing" aria-live="polite">
    {#if playing}
      <div class="line">
        <StatusPill tone={playing.paused ? "neutral" : "live"}>{playing.paused ? soundWords.paused : playing.ending ? soundWords.fading : soundWords.playing}</StatusPill>
        <span class="name">{playing.name}</span>
      </div>
      <div class="bar" role="progressbar" aria-label={playing.name} aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.round(progressFraction(playing, link.soundAt, now) * 100)}>
        <span style:width="{progressFraction(playing, link.soundAt, now) * 100}%"></span>
      </div>
      <span class="muted small">{progressLabel(playing, link.soundAt, now)}</span>
    {:else}
      <p class="muted">{soundWords.quiet}</p>
    {/if}
  </div>
  <div class="transport">
    {#if can.resume}
      <button type="button" onclick={() => onsound("Resume")}><Icon name="play" size={18} />{soundWords.resume}</button>
    {:else}
      <button type="button" disabled={!can.pause} onclick={() => onsound("Pause")}>{soundWords.pause}</button>
    {/if}
    <button type="button" disabled={!can.fadeOut} onclick={() => onsound("Stop")}>{soundWords.fadeOut}</button>
  </div>
  {#if queue.waiting}
    <ul class="plain sounds">{@render soundRow(queue.waiting, "waiting")}</ul>
  {/if}
  {#if sounds.length > 0}
    {#if all}
      {#if queue.played.length > 0}
        <h3>{soundWords.played(queue.played.length)}</h3>
        <ul class="plain sounds">
          {#each queue.played as s (entryKey(s))}{@render soundRow(s, "played")}{/each}
        </ul>
      {/if}
      {#if queue.rest.some((s) => s !== queue.waiting)}
        <h3>{soundWords.inService}</h3>
        <ul class="plain sounds">
          {#each queue.rest.filter((s) => s !== queue.waiting) as s (entryKey(s))}{@render soundRow(s, "")}{/each}
        </ul>
      {/if}
    {:else if queue.upNext}
      <h3>{soundWords.upNext}</h3>
      <ul class="plain sounds">{@render soundRow(queue.upNext, "")}</ul>
    {/if}
    <button type="button" class="more" aria-expanded={all} onclick={() => (all = !all)}>{all ? soundWords.showLess : soundWords.showAll(sounds.length)}</button>
  {/if}
</section>

{#snippet soundRow(s: RemoteSound, kind: "waiting" | "played" | "")}
  <li class:current={playing?.item === s.item} class:waiting={kind === "waiting"} class:played={kind === "played"}>
    <div class="text">
      <span class="name">{s.name}</span>
      <span class="muted small">{kind === "waiting" ? `${soundWords.waiting} · ${s.detail}` : s.detail}</span>
    </div>
    <button type="button" aria-label="{soundWords.play} {s.name}" onclick={() => onsound({ Play: { item: s.item, backing: s.backing } })}>
      <Icon name="play" size={18} />{soundWords.play}
    </button>
  </li>
{/snippet}

<style>
  .sound {
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
    margin: 0;
    font-size: 0.85rem;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }
  .where,
  .small {
    font-size: 0.8rem;
  }
  .warning {
    display: flex;
    gap: 0.5rem;
    align-items: flex-start;
    font-size: 0.9rem;
    color: var(--warn);
  }
  .playing {
    display: grid;
    gap: 0.35rem;
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
    height: 5px;
    border-radius: 3px;
    background: var(--line);
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--gold);
  }
  .transport {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.5rem;
  }
  .sounds {
    display: grid;
    gap: 0.35rem;
    border-top: 1px solid var(--line);
    padding-top: 0.5rem;
  }
  .sounds li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.6rem;
    padding: 0.2rem 0.3rem;
    border-radius: var(--radius);
  }
  .sounds li.current {
    outline: 1px solid var(--gold);
  }
  .sounds li.waiting {
    background: var(--warn-soft);
    outline: 1px solid var(--warn);
  }
  .sounds li.played .text {
    opacity: 0.6;
  }
  h3 {
    font-size: 0.8rem;
    font-weight: 600;
    color: var(--muted);
    margin: 0.2rem 0 0;
  }
  .sound .more {
    justify-self: start;
    background: none;
    border: none;
    padding: 0;
    text-decoration: underline;
  }
  .sounds .text {
    display: grid;
    min-width: 0;
  }
</style>
