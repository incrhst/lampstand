<!-- The phone remote (T100): a helper's phone held upright, from the top:
     what is on screen now, what is next, big Back and Next for a thumb, a
     quieter row of Black, Logo and Clear, then the Sound box. Nothing to plan
     or edit: Plan stays on the laptop. When the link drops, everything the
     laptop said goes and the page says it is not connected. -->
<script lang="ts">
  import type { Command, Cursor, Request, SoundRequest } from "../lib/api";
  import {
    words as soundWords,
    notice,
    progressFraction,
    progressLabel,
    transport,
    whereLine,
  } from "../lib/audio.ts";
  import Dot from "../lib/kit/Dot.svelte";
  import Icon from "../lib/kit/Icon.svelte";
  import SlidePreview from "../lib/kit/SlidePreview.svelte";
  import StatusPill from "../lib/kit/StatusPill.svelte";
  import { onScreen, pictureKey, remoteSounds, slideLine, upNext, wantsPicture, words } from "../lib/remote.ts";
  import { link } from "./link.svelte";
  import { keepAwake } from "./wake";

  let online = $derived(link.state === "online");
  let loaded = $derived(link.loaded);
  let shown = $derived(onScreen(link.loaded, link.live));
  let coming = $derived(upNext(link.loaded, link.live));
  let overlay = $derived(link.live?.overlay ?? "None");
  let sounds = $derived(remoteSounds(link.loaded));
  let playing = $derived(link.sound?.playing ?? null);
  let can = $derived(transport(playing));
  let warning = $derived(notice(link.sound));
  let where = $derived(whereLine(link.sound));
  let showSound = $derived(online && (sounds.length > 0 || playing != null));

  // The position moves on by itself between what the laptop says.
  let now = $state(Date.now());
  $effect(() => {
    if (!playing || playing.paused) return;
    const timer = setInterval(() => (now = Date.now()), 500);
    return () => clearInterval(timer);
  });

  // Pictures of slides with no text (deck slides, pictures), drawn by the
  // laptop, kept for as long as the service stays loaded.
  let pictures: Record<string, string | null> = $state({});
  let picturesOf = "";
  async function fetchPicture(cursor: Cursor) {
    if (!loaded) return;
    const key = pictureKey(loaded, cursor);
    if (key in pictures) return;
    pictures[key] = null;
    const width = Math.min(960, Math.round(Math.min(window.innerWidth, 600) * (window.devicePixelRatio || 1)));
    try {
      const r = await link.send({
        area: "Service",
        request: { SlideThumbnail: { item: cursor.item, slide: cursor.slide, step: cursor.step, width } },
      });
      if (r.type === "Thumbnail") pictures[key] = r.data;
    } catch {
      // No picture: the slide's name shows instead.
    }
  }
  const pictureFor = (cursor: Cursor | undefined) => (loaded && cursor ? (pictures[pictureKey(loaded, cursor)] ?? null) : null);
  $effect(() => {
    // A different service (or none) starts the pictures afresh.
    const service = loaded?.service ?? "";
    if (service !== picturesOf) {
      picturesOf = service;
      pictures = {};
    }
    if (!online) return;
    if (shown && wantsPicture(shown.slide)) void fetchPicture(shown.cursor);
    if (coming && wantsPicture(coming.slide)) void fetchPicture(coming.cursor);
  });

  /** What the laptop refused or could not do, in its words, for a few seconds. */
  let problem: string | null = $state(null);
  let problemTimer: ReturnType<typeof setTimeout> | undefined;
  async function act(request: Request) {
    keepAwake();
    navigator.vibrate?.(8);
    problem = null;
    clearTimeout(problemTimer);
    try {
      await link.send(request);
    } catch (e) {
      problem = e instanceof Error ? e.message : String(e);
      problemTimer = setTimeout(() => (problem = null), 6000);
    }
  }
  const live = (command: Command) => act({ area: "Live", request: command });
  const sound = (request: SoundRequest) => act({ area: "Sound", request });
</script>

<main class="remote" class:offline={!online}>
  <header class="top">
    <span class="link" role="status">
      <Dot tone={online ? "ready" : "warn"} size={8} />
      {online ? words.online : link.state === "connecting" ? words.connecting : words.offline}
    </span>
    {#if loaded}<strong class="service" title={loaded.title}>{loaded.title}</strong>{/if}
  </header>

  {#if !online && link.state === "offline"}
    <p class="banner" role="alert">{words.offlineLong}</p>
  {/if}

  <div class="screens">
    <section class="now" aria-label={words.onScreen}>
      <h2>
        {#if online && shown}<Dot tone="live" size={8} />{/if}
        {words.onScreen}
      </h2>
      {#if online && loaded}
        <SlidePreview
          lines={shown && !link.live?.idle ? shown.slide.lines : []}
          footer={shown?.slide.footer ?? null}
          image={shown && !link.live?.idle && wantsPicture(shown.slide) ? pictureFor(shown.cursor) : null}
          {overlay}
          size="large"
          empty={shown ? (shown.slide.label ?? "") : ""}
        />
        <p class="what">
          {#if overlay === "Black"}{words.screenBlack}{:else if overlay === "Logo"}{words.screenLogo}{:else if shown}{slideLine(shown.item.title, shown.slide)}{:else}{words.nothingOnScreen}{/if}
        </p>
      {:else}
        <div class="blank">{online ? words.nothingLoaded : words.offline}</div>
      {/if}
    </section>

    {#if online && loaded}
      <section class="next" aria-label={words.upNext}>
        <div class="mini">
          {#if coming}
            <SlidePreview
              lines={coming.slide.lines}
              image={wantsPicture(coming.slide) ? pictureFor(coming.cursor) : null}
              size="small"
              empty={coming.slide.label ?? ""}
            />
          {/if}
        </div>
        <div class="about">
          <h2>{words.upNext}</h2>
          <p>{coming ? (coming.build ? words.nextBuild : slideLine(coming.title, coming.slide)) : words.end}</p>
        </div>
      </section>
    {/if}
  </div>

  <div class="controls">
    <div class="drive">
      <button type="button" class="back" disabled={!online || !loaded} onclick={() => live("Previous")}>
        <Icon name="prev" size={28} />{words.back}
      </button>
      <button type="button" class="go" disabled={!online || !loaded} onclick={() => live("Next")}>
        {words.next}<Icon name="next" size={28} />
      </button>
    </div>
    <p class="problem" role="alert">{problem ?? ""}</p>

    <div class="quiet">
      <button type="button" disabled={!online} aria-pressed={overlay === "Black"} onclick={() => live("ToggleBlack")}>{words.black}</button>
      <button type="button" disabled={!online} aria-pressed={overlay === "Logo"} onclick={() => live("ToggleLogo")}>{words.logo}</button>
      <button type="button" disabled={!online} title={words.clearHint} onclick={() => live("ClearText")}>{words.clear}</button>
    </div>

    {#if showSound}
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
            <button type="button" onclick={() => sound("Resume")}><Icon name="play" size={18} />{soundWords.resume}</button>
          {:else}
            <button type="button" disabled={!can.pause} onclick={() => sound("Pause")}>{soundWords.pause}</button>
          {/if}
          <button type="button" disabled={!can.fadeOut} onclick={() => sound("Stop")}>{soundWords.fadeOut}</button>
        </div>
        <button type="button" class="panic" onclick={() => sound("Panic")}>{soundWords.panic}</button>
        {#if sounds.length > 0}
          <ul class="plain sounds">
            {#each sounds as s (s.item + (s.backing ? "-track" : ""))}
              <li class:current={playing?.item === s.item}>
                <div class="text">
                  <span class="name">{s.name}</span>
                  <span class="muted small">{s.detail}</span>
                </div>
                <button type="button" aria-label="{soundWords.play} {s.name}" onclick={() => sound({ Play: { item: s.item, backing: s.backing } })}>
                  <Icon name="play" size={18} />{soundWords.play}
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      </section>
    {/if}
  </div>
</main>

<style>
  .remote {
    box-sizing: border-box;
    min-height: 100dvh;
    max-width: 40rem;
    margin: 0 auto;
    padding: max(0.6rem, env(safe-area-inset-top)) max(0.75rem, env(safe-area-inset-right))
      max(1rem, env(safe-area-inset-bottom)) max(0.75rem, env(safe-area-inset-left));
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    -webkit-tap-highlight-color: transparent;
  }
  .top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.75rem;
    min-height: 1.75rem;
  }
  .link {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.85rem;
    color: var(--muted);
    white-space: nowrap;
  }
  .service {
    font-size: 0.9rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .banner {
    border: 1px solid color-mix(in srgb, var(--warn) 35%, transparent);
    border-radius: var(--radius-lg);
    background: var(--warn-soft);
    color: var(--text);
    padding: 0.6rem 0.8rem;
    font-size: 0.9rem;
  }
  h2 {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.75rem;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--muted);
    margin-bottom: 0.35rem;
  }
  .screens {
    display: grid;
    gap: 0.75rem;
  }
  .now :global(.screen) {
    border-radius: var(--radius);
    outline: 2px solid var(--live);
    outline-offset: 0;
  }
  .offline .now :global(.screen) {
    outline-color: var(--line);
  }
  .what {
    margin-top: 0.4rem;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .blank {
    aspect-ratio: 16 / 9;
    display: flex;
    align-items: center;
    justify-content: center;
    text-align: center;
    padding: 1rem;
    border: 1px dashed var(--line-strong);
    border-radius: var(--radius);
    color: var(--muted);
  }
  .next {
    display: grid;
    grid-template-columns: 40% 1fr;
    gap: 0.75rem;
    align-items: center;
  }
  .mini :global(.screen) {
    border-radius: var(--radius);
  }
  .next p {
    overflow: hidden;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
  }
  .controls {
    display: grid;
    gap: 0.6rem;
    align-content: start;
  }
  button {
    font: inherit;
    color: var(--text);
    background: var(--raised);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    min-height: 3rem;
    padding: 0 0.9rem;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 0.5rem;
    cursor: pointer;
    touch-action: manipulation;
    user-select: none;
  }
  button:active:not(:disabled) {
    background: var(--line-strong);
  }
  button:disabled {
    opacity: 0.45;
    filter: saturate(0);
  }
  .drive {
    display: grid;
    grid-template-columns: 1fr 2fr;
    gap: 0.6rem;
  }
  .drive button {
    min-height: clamp(5.5rem, 15vh, 8rem);
    font-size: 1.35rem;
    font-weight: 600;
  }
  .drive .go {
    background: var(--gold);
    border-color: var(--gold);
    color: var(--on-gold);
  }
  .drive .go:active:not(:disabled) {
    background: var(--gold-hover);
  }
  .problem {
    min-height: 1.2em;
    font-size: 0.9rem;
    color: var(--warn);
  }
  .problem:empty {
    min-height: 0;
  }
  .quiet {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 0.5rem;
  }
  .quiet button {
    color: var(--muted);
  }
  .quiet button[aria-pressed="true"] {
    background: var(--live-strong);
    border-color: var(--live-strong);
    color: white;
  }
  .sound {
    margin-top: 0.4rem;
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    background: var(--panel);
    padding: 0.75rem;
    display: grid;
    gap: 0.6rem;
  }
  .sound .head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.5rem;
  }
  .sound h2 {
    margin: 0;
    color: var(--text);
    font-size: 0.85rem;
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
  .panic {
    background: color-mix(in srgb, var(--live-strong) 18%, var(--raised));
    border-color: color-mix(in srgb, var(--live) 45%, transparent);
    font-weight: 600;
  }
  .panic:active {
    background: var(--live-strong);
    color: white;
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
  .sounds .text {
    display: grid;
    min-width: 0;
  }

  /* A tablet, or a phone turned on its side: what is shown on the left, the
     controls on the right. */
  @media (orientation: landscape) and (min-width: 560px) {
    .remote {
      max-width: 72rem;
      display: grid;
      grid-template-columns: minmax(0, 1.2fr) minmax(0, 1fr);
      grid-template-rows: auto auto 1fr;
      grid-template-areas: "top top" "banner banner" "screens controls";
      column-gap: 1.25rem;
      align-items: start;
    }
    .top {
      grid-area: top;
    }
    .banner {
      grid-area: banner;
    }
    .screens {
      grid-area: screens;
    }
    .controls {
      grid-area: controls;
    }
  }
</style>
