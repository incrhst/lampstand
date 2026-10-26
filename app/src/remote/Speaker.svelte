<!-- A speaker's phone (0.9.1), held upright: "Your slides", the item's
     title, a timer since it went on air and the clock; then either "Waiting
     for your slides" (what is on the screen now, buttons greyed) or the
     slide on screen, large, what the next click shows, and the slide's
     notes; Back and Next at the foot. No wall, no Idle, no sound and no
     other items: the laptop's gate lets this phone send only Next, Back and
     Blank while its item is on air, and never past either end of it. -->
<script lang="ts">
  import type { Command, Cursor } from "../lib/api";
  import Icon from "../lib/kit/Icon.svelte";
  import SlidePreview from "../lib/kit/SlidePreview.svelte";
  import { pictureKey, wantsPicture, words } from "../lib/remote.ts";
  import { PictureQueue, serviceShape } from "../lib/remoteWall.ts";
  import { clock, elapsed, liveSince, speakerView, speakerWords } from "../lib/speaker.ts";
  import { link } from "./link.svelte";
  import { keepAwake } from "./wake";

  let { item, title }: { item: string; title: string } = $props();

  let online = $derived(link.state === "online");
  let view = $derived(speakerView(link.loaded, link.live, item));
  let onAir = $derived(online && view.state === "live");
  let heading = $derived(view.state === "gone" ? title : view.title);

  // The timer runs from when the item went on air; it starts again each time.
  let since: number | null = $state(null);
  let now = $state(Date.now());
  $effect(() => {
    const timer = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(timer);
  });
  $effect(() => {
    const next = liveSince(since, onAir, Date.now());
    if (next !== since) since = next;
  });

  // Pictures of its slides, at the step on screen and the next click.
  let pictures: Record<string, string> = $state({});
  const queue = new PictureQueue<{ cursor: Cursor; width: number }>(
    async ({ cursor, width }) => {
      const r = await link.send({ area: "Service", request: { SlideThumbnail: { ...cursor, width } } });
      return r.type === "Thumbnail" ? r.data : null;
    },
    (key, picture) => (pictures[key] = picture),
  );
  const big = Math.min(960, Math.round(Math.min(typeof window === "undefined" ? 390 : window.innerWidth, 600) * (globalThis.devicePixelRatio || 1)));
  const keyOf = (cursor: Cursor, width: number) => (link.loaded ? `${pictureKey(link.loaded, cursor)}@${width}` : "");
  const pictureAt = (cursor: Cursor, width: number) => pictures[keyOf(cursor, width)] ?? null;
  let shape = "";
  $effect(() => {
    const s = serviceShape(link.loaded);
    if (s !== "" && s !== shape) {
      shape = s;
      queue.reset();
      pictures = {};
    }
  });
  let wasOnline = false;
  $effect(() => {
    if (online && !wasOnline) queue.retry();
    wasOnline = online;
  });
  $effect(() => {
    if (!online || view.state !== "live") return;
    if (wantsPicture(view.slide)) queue.want(keyOf(view.cursor, big), { cursor: view.cursor, width: big }, 0);
    const next = view.next;
    if (next && wantsPicture(next.slide)) queue.want(keyOf(next.cursor, 240), { cursor: next.cursor, width: 240 }, 1);
  });

  /** What the laptop refused, in its words, for a few seconds. */
  let problem: string | null = $state(null);
  let problemTimer: ReturnType<typeof setTimeout> | undefined;
  async function press(command: Command) {
    keepAwake();
    navigator.vibrate?.(8);
    problem = null;
    clearTimeout(problemTimer);
    try {
      await link.send({ area: "Live", request: command });
    } catch (e) {
      problem = e instanceof Error ? e.message : String(e);
      problemTimer = setTimeout(() => (problem = null), 6000);
    }
  }
</script>

<main class="speaker" class:offline={!online}>
  <header class="top">
    <div class="who">
      <span class="eyebrow">{speakerWords.yourSlides}</span>
      <strong class="title">{heading}</strong>
      <span class="link" role="status">
        <span class="link-dot" class:on={online}></span>{online ? words.online : link.state === "connecting" ? words.connecting : words.offline}
      </span>
    </div>
    <div class="time">
      <span class="timer" class:running={since !== null} role="timer" aria-label="Time on the screen">{since !== null ? elapsed(now - since) : "0:00"}</span>
      <span class="clock">{clock(new Date(now))}</span>
    </div>
  </header>

  {#if !online && link.state === "offline"}<p class="banner" role="alert">{words.offlineLong}</p>{/if}

  <section class="body">
    {#if !online}
      <div class="waiting"><p class="muted">{words.offline}</p></div>
    {:else if view.state === "gone"}
      <div class="waiting"><p class="muted">{link.loaded ? speakerWords.gone : words.nothingLoaded}</p></div>
    {:else if view.state === "waiting"}
      <div class="waiting">
        <p class="state"><span class="dot"></span>{speakerWords.waiting}</p>
        <p>{speakerWords.waitingLead(view.title)}</p>
        <p class="muted">{view.onScreen ? speakerWords.onScreenNow(view.onScreen) : speakerWords.nothingOnScreen}</p>
      </div>
    {:else}
      <div class="status">
        <p class="state" class:live={!view.blank}>
          <span class="dot"></span><strong>{view.blank ? speakerWords.blank : speakerWords.live}</strong>
          <span class="position">{view.position}</span>
        </p>
        <button type="button" class="blank-button" aria-pressed={view.blank} onclick={() => press("ToggleBlack")}>{speakerWords.blankButton}</button>
      </div>
      <div class="screen" class:live={!view.blank}>
        <SlidePreview
          lines={view.slide.lines}
          footer={view.slide.footer}
          image={wantsPicture(view.slide) ? pictureAt(view.cursor, big) : null}
          overlay={view.blank ? "Black" : "None"}
          size="large"
          empty={view.slide.label ?? ""}
        />
      </div>
      {#if view.next}
        <div class="next">
          <div class="mini">
            <SlidePreview
              lines={view.next.slide.lines}
              image={wantsPicture(view.next.slide) ? pictureAt(view.next.cursor, 240) : null}
              size="small"
              empty={view.next.slide.label ?? ""}
            />
          </div>
          <div class="about">
            <strong>{view.next.kind === "click" ? speakerWords.nextClick : speakerWords.nextSlide}</strong>
            <span class="muted">{view.next.kind === "click" ? speakerWords.clicksLeft(view.next.clicksLeft) : view.next.title}</span>
          </div>
        </div>
      {:else}
        <p class="last">{speakerWords.last}</p>
      {/if}
      <div class="notes" aria-label={speakerWords.notes}>
        {#if view.notes.trim()}{view.notes}{:else}<span class="muted">{speakerWords.noNotes}</span>{/if}
      </div>
    {/if}
  </section>

  <footer class="bottom">
    <p class="problem" role="alert">{problem ?? ""}</p>
    <div class="drive">
      <button type="button" class="back" disabled={!onAir || (view.state === "live" && view.first)} onclick={() => press("Previous")}>
        <Icon name="prev" size={26} />{speakerWords.back}
      </button>
      <button type="button" class="go" disabled={!onAir || (view.state === "live" && view.last)} onclick={() => press("Next")}>
        {speakerWords.next}<Icon name="next" size={26} />
      </button>
    </div>
  </footer>
</main>

<style>
  .speaker {
    box-sizing: border-box;
    height: 100dvh;
    max-width: 40rem;
    margin: 0 auto;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    -webkit-tap-highlight-color: transparent;
  }
  button {
    font: inherit;
    color: var(--text);
    background: var(--raised);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    min-height: max(44px, 2.75rem);
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
    opacity: 0.4;
    filter: saturate(0);
  }
  .top {
    flex-shrink: 0;
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 0.75rem;
    padding: max(0.6rem, env(safe-area-inset-top)) max(0.9rem, env(safe-area-inset-right)) 0.6rem max(0.9rem, env(safe-area-inset-left));
    border-bottom: 1px solid var(--line);
  }
  .who {
    display: grid;
    gap: 0.1rem;
    min-width: 0;
  }
  .eyebrow {
    font-size: 0.7rem;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--muted);
  }
  .title {
    font-size: 1.1rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .link {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    font-size: 0.75rem;
    color: var(--muted);
  }
  .link-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--warn);
  }
  .link-dot.on {
    background: var(--ready);
  }
  .time {
    display: grid;
    justify-items: end;
    flex-shrink: 0;
  }
  .timer {
    font-size: 2.1rem;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
    line-height: 1.05;
    color: var(--faint);
  }
  .timer.running {
    color: var(--text);
  }
  .clock {
    font-size: 0.85rem;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .banner {
    flex-shrink: 0;
    margin: 0.5rem 0.9rem 0;
    border: 1px solid color-mix(in srgb, var(--warn) 35%, transparent);
    border-radius: var(--radius-lg);
    background: var(--warn-soft);
    padding: 0.6rem 0.8rem;
    font-size: 0.9rem;
  }
  .body {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
    padding: 0.7rem max(0.9rem, env(safe-area-inset-right)) 0.4rem max(0.9rem, env(safe-area-inset-left));
  }
  .waiting {
    margin: auto 0;
    display: grid;
    gap: 0.6rem;
    text-align: center;
    padding: 1.5rem 0.5rem;
    font-size: 1rem;
  }
  .state {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 0.45rem;
    font-weight: 600;
  }
  .dot {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: var(--faint);
    flex-shrink: 0;
  }
  .state.live .dot {
    background: var(--live);
  }
  .state.live strong {
    color: var(--live);
  }
  .status {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
  }
  .status .state {
    justify-content: flex-start;
    font-size: 0.9rem;
    min-width: 0;
  }
  .position {
    font-weight: 400;
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .blank-button {
    color: var(--muted);
    min-height: max(44px, 2.75rem);
    font-size: 0.85rem;
  }
  .blank-button[aria-pressed="true"] {
    background: var(--live-strong);
    border-color: var(--live-strong);
    color: white;
  }
  .screen {
    flex-shrink: 0;
    border-radius: var(--radius);
    overflow: hidden;
    box-shadow: 0 0 0 2px var(--line);
  }
  .screen.live {
    box-shadow: 0 0 0 3px var(--live);
  }
  .next {
    flex-shrink: 0;
    display: grid;
    grid-template-columns: 34% 1fr;
    gap: 0.75rem;
    align-items: center;
  }
  .mini {
    border-radius: var(--radius);
    overflow: hidden;
    box-shadow: 0 0 0 1px var(--line-strong);
  }
  .about {
    display: grid;
    gap: 0.15rem;
    font-size: 0.9rem;
    min-width: 0;
  }
  .last {
    flex-shrink: 0;
    color: var(--gold);
    font-weight: 600;
  }
  .notes {
    flex: 1;
    min-height: max(48px, 3rem);
    overflow-y: auto;
    overscroll-behavior: contain;
    font-size: 19px;
    line-height: 1.45;
    white-space: pre-wrap;
    border-top: 1px solid var(--line);
    padding-top: 0.6rem;
  }
  .bottom {
    flex-shrink: 0;
    padding: 0.4rem max(0.9rem, env(safe-area-inset-right)) max(0.7rem, env(safe-area-inset-bottom)) max(0.9rem, env(safe-area-inset-left));
    border-top: 1px solid var(--line);
    display: grid;
    gap: 0.4rem;
  }
  .problem {
    font-size: 0.85rem;
    color: var(--warn);
  }
  .problem:empty {
    display: none;
  }
  .drive {
    display: grid;
    grid-template-columns: 1fr 2fr;
    gap: 0.6rem;
  }
  .drive button {
    min-height: 76px;
    font-size: 1.3rem;
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
</style>
