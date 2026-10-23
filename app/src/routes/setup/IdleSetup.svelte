<!-- Setup › Idle screen: what the audience screens show when nothing is on
     air, before the service, between items and after it. One choice for every
     screen, and a screen that needs its own can have one. Logo and Blank in
     Present still cover it. -->
<script lang="ts">
  import { request, type Deck, type OutputConfig } from "../../lib/api";
  import { describeIdle } from "../../lib/idle";
  import { isScreen } from "../../lib/obs";
  import { live, refreshOutputs, refreshSettings, report, saveSettings } from "../../lib/state.svelte";
  import IdlePicker from "../../lib/kit/IdlePicker.svelte";
  import Panel from "../../lib/kit/Panel.svelte";
  import SlidePreview from "../../lib/kit/SlidePreview.svelte";

  let { onscreens }: { onscreens?: () => void } = $props();

  /** Audience screens only: a stage display or a stream never shows the idle screen. */
  let screens = $derived(live.outputs.filter((o) => isScreen(o) && typeof o.role === "object"));
  let decks: Deck[] = $state([]);

  async function put(config: OutputConfig) {
    try {
      live.outputs = await request({ area: "Outputs", request: { Put: config } }, "Outputs");
    } catch (e) {
      report(e);
    }
  }

  refreshOutputs();
  refreshSettings();
  request({ area: "Library", request: "ListDecks" }, "Decks")
    .then((d) => (decks = d))
    .catch(() => {});
</script>

<div class="page">
  <h1>Idle screen</h1>
  <p class="muted intro">
    What the screens show when nothing is on air: before the service, between items and after it. Choose the plain
    theme background, a picture, a video that loops with the sound off, or one slide of a deck. Press <kbd>Logo</kbd>
    or <kbd>Blank</kbd> in Present to cover it.
  </p>

  <Panel title="All screens" description="Used by every screen that has not chosen its own.">
    <div class="current">
      <SlidePreview lines={[]} size="small" empty={describeIdle(live.settings?.idle ?? null, decks, "The theme's plain background")} />
    </div>
    <IdlePicker label="Idle screen for all screens" value={live.settings?.idle ?? "Blank"} onchange={(idle) => saveSettings({ idle: idle === "Blank" ? null : idle })} />
  </Panel>

  <h2>Each screen</h2>
  {#if screens.length === 0}
    <p class="muted">No audience screens yet. {#if onscreens}<button class="link" onclick={onscreens}>Add one under Screens</button>.{/if}</p>
  {/if}
  <ul class="plain">
    {#each screens as o (o.id)}
      <li>
        <div class="who">
          <strong>{o.name}</strong>
          <span class="muted">{describeIdle(o.idle, decks, "Same as all screens")}</span>
        </div>
        <IdlePicker label="Idle screen for {o.name}" allowInherit value={o.idle} onchange={(idle) => put({ ...o, idle })} />
      </li>
    {/each}
  </ul>
</div>

<style>
  .page {
    max-width: 52rem;
    margin: 0 auto;
    padding: 2rem;
  }
  h1 {
    font-size: 1.3rem;
    font-weight: 600;
    letter-spacing: -0.01em;
  }
  h2 {
    margin: 1.75rem 0 0.5rem;
    font-size: 1rem;
    font-weight: 600;
  }
  .intro {
    margin: 0.3rem 0 1.5rem;
    max-width: 40rem;
    font-size: 0.875rem;
    line-height: 1.6;
  }
  .current {
    width: 16rem;
    margin-bottom: 0.75rem;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    overflow: hidden;
  }
  li {
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
    padding: 1rem 0;
    border-bottom: 1px solid var(--line);
  }
  .who {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    font-size: 0.875rem;
  }
  .link {
    font: inherit;
    padding: 0;
    background: none;
    border: none;
    color: var(--text);
    text-decoration: underline;
    cursor: pointer;
  }
  kbd {
    font: inherit;
    font-size: 0.8125rem;
    padding: 0.05rem 0.35rem;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius);
  }
</style>
