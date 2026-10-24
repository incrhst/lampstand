<!-- Outputs: this device's screens, each with a preview inside its safe
     area, and the displays a fullscreen output can go on.
     T50 replaces this with outputs setup and the calibration wizard. -->
<script lang="ts">
  import { request, type DisplayMatch, type OutputConfig } from "../../lib/api";
  import { isCalibrated, live, refreshOutputs, refreshSettings, refreshThemes, report } from "../../lib/state.svelte";
  import { isScreen } from "../../lib/obs";
  import Button from "../../lib/kit/Button.svelte";
  import Icon from "../../lib/kit/Icon.svelte";
  import Banner from "../../lib/kit/Banner.svelte";
  import { steppedDown, takesMainPlace, windowToDemote } from "../../lib/primary";
  import SlidePreview from "../../lib/kit/SlidePreview.svelte";
  import { displaysWithScreens } from "../../lib/screenOffers";
  import Segmented from "../../lib/kit/Segmented.svelte";
  import Switch from "../../lib/kit/Switch.svelte";
  import CalibrateView from "./CalibrateView.svelte";
  import ThemePicker from "../../lib/kit/ThemePicker.svelte";
  import PresentAction from "../../lib/kit/PresentAction.svelte";
  import { isPlanning, PLANNING_SCREENS } from "../../lib/presenting";

  /** Opens Setup › OBS, where pictures sent to OBS are set up. */
  let { onobs, onidle }: { onobs?: () => void; onidle?: () => void } = $props();

  /** The screens: windows and displays. Pictures sent to OBS are on the OBS page. */
  let screens = $derived(live.outputs.filter(isScreen));

  let displays: DisplayMatch[] = $state([]);
  /** The output being calibrated, and the one just saved (for the notice). */
  let calibrating: string | null = $state(null);
  let saved: string | null = $state(null);
  let calibratingOutput = $derived(live.outputs.find((o) => o.id === calibrating) ?? null);
  let savedOutput = $derived(live.outputs.find((o) => o.id === saved) ?? null);

  async function saveSafeArea(o: OutputConfig, safe_area: OutputConfig["safe_area"], background: OutputConfig["background"]) {
    await put({ ...o, safe_area, background });
    calibrating = null;
    saved = o.id;
  }

  const hasMargins = isCalibrated;

  const sample = ["Amazing grace! How sweet the sound", "That saved a wretch like me!"];

  async function refresh() {
    await refreshOutputs();
    try {
      displays = await request({ area: "Outputs", request: "ListDisplays" }, "Displays");
    } catch (e) {
      report(e);
    }
  }

  // A monitor plugged in or removed shows up here by itself; the Refresh
  // button is only a backstop.
  let lastDisplays = live.displaysVersion;
  $effect(() => {
    if (live.displaysVersion === lastDisplays) return;
    lastDisplays = live.displaysVersion;
    refresh();
  });

  async function put(config: OutputConfig) {
    try {
      live.outputs = await request({ area: "Outputs", request: { Put: config } }, "Outputs");
    } catch (e) {
      report(e);
    }
  }

  async function remove(id: string) {
    try {
      live.outputs = await request({ area: "Outputs", request: { Remove: id } }, "Outputs");
    } catch (e) {
      report(e);
    }
  }

  async function add(name: string, target: OutputConfig["target"], stream = false) {
    const primary = !stream && takesMainPlace(live.outputs, target);
    // A display takes the main place from the rehearsal window.
    const window = stream ? null : windowToDemote(live.outputs, target);
    if (window) await put(steppedDown(window));
    await put({
      id: crypto.randomUUID(),
      name,
      role: stream ? "Stream" : { Audience: { primary } },
      target,
      safe_area: { top: 0, right: 0, bottom: 0, left: 0 },
      background: "Bleed",
      theme: null,
      idle: null,
      enabled: false,
    });
  }

  function describe(o: OutputConfig): string {
    const t = o.target;
    if ("Window" in t) return `Window · ${t.Window.width}×${t.Window.height}`;
    if ("Display" in t) return `Fullscreen on ${t.Display.name || "display"} · ${t.Display.width}×${t.Display.height}`;
    return `Sent to OBS · ${t.BrowserSource.width}×${t.BrowserSource.height}`;
  }

  function role(o: OutputConfig): string {
    if (typeof o.role === "object") return o.role.Audience.primary ? "Main audience screen" : "Audience screen";
    return o.role === "Stage" ? "Stage display" : "Stream";
  }

  function margins(o: OutputConfig): string {
    const s = o.safe_area;
    const all = [s.top, s.right, s.bottom, s.left];
    if (all.every((m) => m === 0)) return "No safe-area margins";
    const fit = o.background === "Fit" ? ", backgrounds inside" : "";
    return `Margins ${all.map((m) => `${Math.round(m * 1000) / 10}%`).join(" ")}${fit}`;
  }

  refresh();
  refreshThemes();
  refreshSettings();
</script>

{#if calibratingOutput}
  <CalibrateView
    output={calibratingOutput}
    onsave={(area, background) => saveSafeArea(calibratingOutput!, area, background)}
    oncancel={() => (calibrating = null)}
  />
{:else}
<div class="outputs">
  <div class="page">
    <div class="head">
      <div>
        <h1>Screens</h1>
        <p class="muted intro">
          The projectors, TVs and windows this device drives. Each keeps lyrics and scripture inside its own safe area. Turn a
          screen on to open its window; fullscreen screens go on a display below. Pictures for OBS are under Setup › OBS.
        </p>
      </div>
      <div class="add">
        {#if onobs}<Button icon="broadcast" onclick={onobs}>Send to OBS…</Button>{/if}
        <Button variant="primary" icon="plus" onclick={() => add("Rehearsal window", { Window: { width: 960, height: 540 } })}>
          Add window
        </Button>
      </div>
    </div>

    {#if isPlanning(live.team)}
      <div class="saved">
        <Banner tone="warn" icon="monitor">
          <span>{PLANNING_SCREENS}</span>
          <PresentAction />
        </Banner>
      </div>
    {/if}

    {#if savedOutput}
      <div class="saved">
        <Banner tone="ready">
          <span>{savedOutput.name}: safe area saved. Lyrics and scripture now stay inside it.</span>
          <Button variant="ghost" size="small" icon="x" label="Dismiss" onclick={() => (saved = null)} />
        </Banner>
      </div>
    {/if}

    {#if onidle}
      <p class="muted idle-link">
        What the screens show when nothing is on air is under
        <button class="link" onclick={onidle}>Setup › Idle screen</button>.
      </p>
    {/if}

    <ul class="plain screens">
      {#each screens as o (o.id)}
        <li class="screen">
          <div class="preview">
            <SlidePreview lines={sample} safeArea={o.safe_area} showGuide size="small" />
          </div>
          <div class="info">
            <h2>{o.name}</h2>
            <p class="muted">{describe(o)}</p>
            <p class="muted">{role(o)} · {margins(o)}</p>
            {#if hasMargins(o)}
              <p class="calibration ok"><Icon name="checkCircle" size={14} />Safe area set</p>
            {:else}
              <p class="calibration warn"><Icon name="alert" size={14} />Not calibrated: text may be cut off at the edges</p>
            {/if}
          </div>
          <div class="options">
            <label class="option">
              <span class="muted">Theme</span>
              <ThemePicker
                label="Theme for {o.name}"
                value={o.theme}
                inherit="Same as the service"
                onchange={(theme) => put({ ...o, theme })}
              />
            </label>
          </div>
          <div class="controls">
            <Switch checked={o.enabled} onchange={(on) => put({ ...o, enabled: on })} label={o.enabled ? "On" : "Off"} />
            <Button variant={hasMargins(o) ? "secondary" : "primary"} size="small" icon="safeArea" onclick={() => (calibrating = o.id)}>
              {hasMargins(o) ? "Recalibrate" : "Calibrate"}
            </Button>
            <Button variant="ghost" size="small" icon="trash" onclick={() => remove(o.id)}>Remove</Button>
          </div>
        </li>
      {:else}
        <li class="empty">
          <Icon name="monitor" size={24} />
          <p><strong>No screens yet</strong></p>
          <p class="muted">Add a window to see slides while you rehearse, or use a display below.</p>
        </li>
      {/each}
    </ul>

    <div class="displays-head">
      <h2 class="title">Displays</h2>
      <Button size="small" onclick={refresh}>Refresh</Button>
    </div>
    <ul class="plain displays">
      {#each displays as d, i (i)}
        {@const hasScreen = displaysWithScreens(live.outputs, displays).has(i)}
        <li>
          <Icon name="monitor" />
          <span class="display-name">{d.name || `Display ${i + 1}`}</span>
          <span class="faint num">{d.width}×{d.height} at {d.x},{d.y}</span>
          {#if hasScreen}
            <Button size="small" disabled onclick={() => {}} title="This display already has a screen above">Already a screen</Button>
          {:else}
            <Button size="small" onclick={() => add(d.name || `Display ${i + 1}`, { Display: d })}>Use as a screen</Button>
          {/if}
        </li>
      {:else}
        <li class="faint">No displays found.</li>
      {/each}
    </ul>
  </div>
</div>
{/if}

<style>
  .outputs {
    height: 100%;
    overflow-y: auto;
  }
  .page {
    max-width: 64rem;
    margin: 0 auto;
    padding: 2rem;
  }
  .head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 1.5rem;
  }
  h1 {
    font-size: 1.3rem;
    font-weight: 600;
    letter-spacing: -0.01em;
  }
  .add {
    display: flex;
    gap: 0.5rem;
    flex-shrink: 0;
  }
  .intro {
    margin-top: 0.3rem;
    max-width: 40rem;
    font-size: 0.875rem;
    line-height: 1.6;
  }
  .screens {
    margin-top: 1.5rem;
    border-top: 1px solid var(--line);
    border-bottom: 1px solid var(--line);
  }
  .screen {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 1.25rem;
    padding: 1.5rem 0;
  }
  .options {
    flex-basis: 100%;
    order: 3;
    display: flex;
    flex-wrap: wrap;
    gap: 1rem 2rem;
    padding-left: 17.25rem;
  }
  .option {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    font-size: 0.8rem;
  }
  .idle-link {
    margin: 1.25rem 0 0;
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
  .screen + .screen {
    border-top: 1px solid var(--line);
  }
  .preview {
    width: 16rem;
    flex-shrink: 0;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    overflow: hidden;
  }
  .info {
    flex: 1;
    min-width: 0;
    font-size: 0.875rem;
  }
  .info h2 {
    font-size: 1rem;
    font-weight: 600;
    margin-bottom: 0.2rem;
  }
  .calibration {
    margin-top: 0.75rem;
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }
  .calibration.ok :global(.icon) {
    color: var(--ready-text);
  }
  .calibration.warn {
    color: var(--warn);
  }
  .saved {
    margin-top: 1.5rem;
  }
  .controls {
    display: flex;
    align-items: center;
    gap: 1rem;
  }
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.4rem;
    padding: 3rem 1rem;
    text-align: center;
    font-size: 0.875rem;
    color: var(--faint);
  }
  .empty strong {
    color: var(--text);
  }
  .displays-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-top: 2rem;
  }
  .displays {
    margin-top: 0.5rem;
  }
  .displays li {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding: 0.75rem 0;
    border-bottom: 1px solid var(--line);
    font-size: 0.875rem;
    color: var(--muted);
  }
  .display-name {
    color: var(--text);
  }
  .displays li :global(button) {
    margin-left: auto;
  }
</style>
