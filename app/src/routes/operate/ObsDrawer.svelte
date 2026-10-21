<!-- Present's OBS drawer (T91): what OBS is sending out now, refreshed a
     few times a second, OBS's scenes as buttons with the one on air
     marked, and a mute toggle for each audio input. Pointing at a scene
     (hover or keyboard focus) previews it below the buttons before
     switching. It isn't
     modal: Space, the arrows and the other Present shortcuts keep
     working while it is open, and Escape closes it. Pictures are only
     asked for while it is open. -->
<script lang="ts">
  import { untrack } from "svelte";
  import { request, type ObsView } from "../../lib/api";
  import { isConnected, notice, pollImages } from "../../lib/obs";
  import { obs, report } from "../../lib/state.svelte";
  import Button from "../../lib/kit/Button.svelte";
  import Icon from "../../lib/kit/Icon.svelte";
  import StatusPill from "../../lib/kit/StatusPill.svelte";

  let { view, onclose, onsetup }: { view: ObsView | null; onclose: () => void; onsetup?: () => void } = $props();

  const PROGRAM_WIDTH = 640;
  const PREVIEW_WIDTH = 400;

  let connected = $derived(isConnected(view));
  let program = $derived(view?.program_scene ?? null);
  let scenes = $derived(view?.scenes ?? []);
  let audio = $derived(view?.audio ?? []);
  let problem = $derived(notice(view));

  // The program picture, while connected.
  let onAir: string | null = $state(null);
  let stale = $state(false);
  $effect(() => {
    if (!connected) {
      untrack(() => (onAir = null));
      return;
    }
    return pollImages(
      () => request({ area: "Obs", request: { Screenshot: { scene: null, width: PROGRAM_WIDTH } } }, "Thumbnail"),
      (image) => {
        onAir = image;
        stale = false;
      },
      undefined,
      () => (stale = true),
    );
  });

  // A scene pointed at, previewed before switching. The program scene
  // is already in the picture above.
  let pointed: string | null = $state(null);
  let preview: { scene: string; image: string } | null = $state(null);
  let previewing = $derived(connected && pointed && pointed !== program ? pointed : null);
  $effect(() => {
    const scene = previewing;
    if (!scene) {
      untrack(() => (preview = null));
      return;
    }
    return pollImages(
      () => request({ area: "Obs", request: { Screenshot: { scene, width: PREVIEW_WIDTH } } }, "Thumbnail"),
      (image) => (preview = { scene, image }),
    );
  });

  // The scene asked for, until OBS says it's on air.
  let switching: string | null = $state(null);
  async function pick(scene: string) {
    if (scene === program) return;
    switching = scene;
    try {
      await obs({ SetScene: { scene } });
    } catch (e) {
      report(e);
    } finally {
      switching = null;
    }
  }

  async function toggleMute(input: string, muted: boolean) {
    try {
      await obs({ SetMute: { input, muted: !muted } });
    } catch (e) {
      report(e);
    }
  }

  // Opening the drawer puts focus on the scene on air, so Tab and Enter
  // reach the other scenes straight away.
  let body: HTMLElement | undefined = $state();
  $effect(() => {
    if (!body) return;
    untrack(() => {
      const target = body?.querySelector<HTMLElement>(".scene[aria-pressed='true']") ?? body?.querySelector<HTMLElement>("button");
      target?.focus({ preventScroll: true });
    });
  });

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onclose();
    }
  }

  // Space is Next everywhere in Present, so it never presses a focused
  // button in here: a scene clicked a moment ago mustn't be picked again
  // (and hold) when the operator presses Space for the next slide.
  function onkeyup(e: KeyboardEvent) {
    if (e.key === " ") e.preventDefault();
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<aside id="obs-drawer" class="drawer" aria-label="Control OBS" bind:this={body} {onkeydown} {onkeyup}>
  <div class="head">
    <h2 class="title">Control OBS</h2>
    <span class="faint small">O or Esc to close</span>
    <Button variant="ghost" size="small" icon="x" label="Close Control OBS" onclick={onclose} />
  </div>

  {#if !connected}
    <div class="offline">
      <p>{problem?.message ?? (view?.connection === "Connecting" ? "Connecting to OBS…" : "OBS isn't connected.")}</p>
      {#if onsetup}<Button size="small" icon="setup" onclick={onsetup}>OBS settings</Button>{/if}
    </div>
  {:else}
    <section aria-label="On air in OBS">
      <div class="label">
        <StatusPill tone="live">ON AIR</StatusPill>
        <span class="name" title={program ?? undefined}>{program ?? "—"}</span>
        {#if stale}<span class="faint small">picture paused</span>{/if}
      </div>
      <div class="screen on">
        {#if onAir}
          <img src={onAir} alt="What OBS is sending out now: {program}" />
        {:else}
          <span class="faint small">Waiting for OBS…</span>
        {/if}
      </div>
    </section>

    <section>
      <h3 class="title">Scenes</h3>
      {#if scenes.length === 0}
        <p class="faint small">OBS has no scenes.</p>
      {:else}
        <ul class="plain scenes">
          {#each scenes as scene (scene)}
            <li>
              <button
                type="button"
                class="scene"
                class:live={scene === program}
                class:switching={scene === switching}
                aria-pressed={scene === program}
                title={scene === program ? `${scene} is on air` : `Switch OBS to ${scene}`}
                onclick={() => pick(scene)}
                onpointerenter={() => (pointed = scene)}
                onpointerleave={() => pointed === scene && (pointed = null)}
                onfocus={() => (pointed = scene)}
                onblur={() => pointed === scene && (pointed = null)}
              >
                <span class="scene-name">{scene}</span>
                {#if scene === program}<span class="dot" aria-hidden="true"></span>{/if}
              </button>
            </li>
          {/each}
        </ul>
      {/if}
    </section>

    <!-- Below the buttons, with its room kept, so pointing at a scene
         never moves the buttons. -->
    <section aria-label="Scene preview" aria-live="polite">
      <div class="label">
        <StatusPill tone="neutral">PREVIEW</StatusPill>
        {#if previewing}
          <span class="name" title={previewing}>{previewing}</span>
          <span class="faint small">click or Enter to switch</span>
        {/if}
      </div>
      <div class="screen">
        {#if previewing && preview?.scene === previewing}
          <img src={preview.image} alt="Scene {previewing}, not on air" />
        {:else if previewing}
          <span class="faint small">Loading…</span>
        {:else}
          <span class="faint small">Point at a scene to see it before it goes on air.</span>
        {/if}
      </div>
    </section>

    {#if audio.length > 0}
      <section>
        <h3 class="title">Audio</h3>
        <ul class="plain audio">
          {#each audio as input (input.name)}
            <li>
              <button
                type="button"
                class="mute"
                class:muted={input.muted}
                aria-pressed={input.muted}
                aria-label="Mute {input.name}"
                title={input.muted ? `${input.name} is muted. Click to unmute` : `Mute ${input.name}`}
                onclick={() => toggleMute(input.name, input.muted)}
              >
                <Icon name={input.muted ? "micOff" : "mic"} size={16} />
                <span class="input-name">{input.name}</span>
                <span class="state small">{input.muted ? "Muted" : "On"}</span>
              </button>
            </li>
          {/each}
        </ul>
      </section>
    {/if}
  {/if}
</aside>

<style>
  .drawer {
    position: absolute;
    inset: 0 0 0 auto;
    z-index: 5;
    width: min(380px, 100%);
    display: flex;
    flex-direction: column;
    gap: 1.25rem;
    padding: 1rem 1.25rem 1.5rem;
    overflow-y: auto;
    background: var(--panel);
    border-left: 1px solid var(--line);
    box-shadow: -12px 0 32px rgba(0, 0, 0, 0.35);
    animation: slide 180ms var(--ease);
  }
  @keyframes slide {
    from {
      transform: translateX(24px);
      opacity: 0;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .drawer {
      animation: none;
    }
  }
  .head {
    display: flex;
    align-items: center;
    gap: 0.75rem;
  }
  .head .small {
    margin-left: auto;
  }
  .small {
    font-size: 0.75rem;
  }
  h3.title {
    margin-bottom: 0.5rem;
  }
  .offline {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.75rem;
    font-size: 0.875rem;
    color: var(--muted);
  }
  .label {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin-bottom: 0.4rem;
    font-size: 0.8125rem;
    min-width: 0;
  }
  .label :global(.pill) {
    padding: 0.05rem 0.4rem;
    font-size: 0.62rem;
  }
  .name {
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .screen {
    aspect-ratio: 16 / 9;
    display: grid;
    place-items: center;
    border-radius: var(--radius);
    overflow: hidden;
    background: #000;
    box-shadow: 0 0 0 1px var(--line-strong);
  }
  .screen.on {
    box-shadow: 0 0 0 2px var(--live);
  }
  .screen img {
    width: 100%;
    height: 100%;
    object-fit: contain;
    display: block;
  }
  .scenes {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 0.5rem;
  }
  .scene,
  .mute {
    font: inherit;
    color: inherit;
    width: 100%;
    display: flex;
    align-items: center;
    gap: 0.5rem;
    border: none;
    border-radius: var(--radius);
    background: var(--raised);
    box-shadow: 0 0 0 1px var(--line);
    cursor: pointer;
    text-align: left;
  }
  .scene {
    min-height: 3rem;
    padding: 0.5rem 0.75rem;
    font-size: 0.875rem;
    font-weight: 500;
  }
  .scene:hover,
  .mute:hover {
    box-shadow: 0 0 0 1px var(--line-strong);
  }
  .scene.live {
    background: color-mix(in srgb, var(--live) 18%, var(--raised));
    box-shadow: 0 0 0 2px var(--live);
    cursor: default;
  }
  .scene.switching {
    box-shadow: 0 0 0 2px var(--gold);
  }
  .scene-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    /* Two lines, then cut: long OBS scene names are common. */
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
  }
  .dot {
    flex-shrink: 0;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--live);
  }
  .audio {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }
  .mute {
    padding: 0.5rem 0.75rem;
    font-size: 0.85rem;
  }
  .input-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .state {
    color: var(--ready-text);
  }
  .mute.muted {
    color: var(--muted);
    box-shadow: 0 0 0 1px var(--live);
  }
  .mute.muted .state {
    color: var(--live);
    font-weight: 600;
  }
</style>
