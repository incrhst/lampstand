<!-- Setup › OBS › Send to OBS: pictures OBS loads from Lampstand as a
     Browser source, either lower thirds for the stream or the full
     screen as the room sees it. OBS pulls these over the network with the
     URL below; it is separate from Lampstand controlling OBS (Control
     OBS), and needs none of that to work. -->
<script lang="ts">
  import { request, type OutputConfig } from "../../lib/api";
  import { isPicture, pictureStatus, withAddress } from "../../lib/obs";
  import { isCalibrated, live, pictures, putOutput, refreshPictures, refreshSettings, refreshThemes, removeOutput, report } from "../../lib/state.svelte";
  import Button from "../../lib/kit/Button.svelte";
  import Dot from "../../lib/kit/Dot.svelte";
  import Icon from "../../lib/kit/Icon.svelte";
  import Segmented from "../../lib/kit/Segmented.svelte";
  import StatusPill from "../../lib/kit/StatusPill.svelte";
  import Switch from "../../lib/kit/Switch.svelte";
  import ThemePicker from "../../lib/kit/ThemePicker.svelte";

  let { oncalibrate }: { oncalibrate: (id: string) => void } = $props();

  let sources = $derived(live.outputs.filter(isPicture));

  refreshThemes();
  refreshSettings();

  // Whether OBS has loaded each URL, checked every few seconds while this
  // page is open.
  $effect(() => {
    refreshPictures();
    const timer = setInterval(refreshPictures, 3000);
    return () => clearInterval(timer);
  });

  /** The URL of each picture, by output ID. */
  let urls: Record<string, string> = $state({});
  let copied: string | null = $state(null);

  /**
   * What a picture shows. One source, one URL in OBS: switching changes its
   * role, so the page OBS already shows redraws in the other layout. Lower
   * thirds is the stream layout (transparent, text in the lower third);
   * full screen is the audience layout, background and all.
   */
  type Serves = "lower" | "full";
  const servesOptions = [
    { id: "lower", label: "Lower thirds" },
    { id: "full", label: "Full screen" },
  ] as const;
  const serves = (o: OutputConfig): Serves => (o.role === "Stream" ? "lower" : "full");
  function setServes(o: OutputConfig, to: Serves) {
    if (to === serves(o)) return;
    putOutput({ ...o, role: to === "lower" ? "Stream" : { Audience: { primary: false } } });
  }

  async function loadUrl(id: string) {
    try {
      urls[id] = await request({ area: "Outputs", request: { BrowserSourceUrl: id } }, "Text");
    } catch (e) {
      report(e);
    }
  }
  $effect(() => {
    for (const o of sources) if (!(o.id in urls)) loadUrl(o.id);
  });

  async function copy(id: string) {
    try {
      await navigator.clipboard.writeText(withAddress(urls[id], live.obs));
      copied = id;
      setTimeout(() => {
        if (copied === id) copied = null;
      }, 2000);
    } catch (e) {
      report(e);
    }
  }

  /** A new key, so a leaked URL stops working. OBS needs the new URL. */
  async function newKey(id: string) {
    try {
      urls[id] = await request({ area: "Outputs", request: { RotateBrowserSourceKey: id } }, "Text");
      copied = null;
    } catch (e) {
      report(e);
    }
  }

  function add() {
    putOutput({
      id: crypto.randomUUID(),
      name: "OBS stream",
      role: "Stream",
      target: { BrowserSource: { width: 1920, height: 1080 } },
      safe_area: { top: 0, right: 0, bottom: 0, left: 0 },
      background: "Bleed",
      theme: null,
      idle: null,
      enabled: false,
    });
  }
</script>

<ul class="plain sources">
  {#each sources as o (o.id)}
    {@const status = pictureStatus(o.enabled, pictures.clients[o.id] ?? 0)}
    <li class="source">
      <div class="info">
        <div class="title-row">
          <Icon name="broadcast" />
          <h3>{o.name}</h3>
          <StatusPill tone={status.tone}>{status.text}</StatusPill>
        </div>
        <div class="serves">
          <Segmented label="What {o.name} shows in OBS" options={servesOptions} value={serves(o)} onchange={(to) => setServes(o, to)} />
        </div>
        <label class="theme">
          <span class="muted">Theme</span>
          <ThemePicker
            label="Theme for {o.name}"
            value={o.theme}
            inherit="Same as the service"
            onchange={(theme) => putOutput({ ...o, theme })}
          />
        </label>
        {#if urls[o.id]}
          <p class="muted small">In OBS, add a <strong>Browser</strong> source with this URL{o.enabled ? "" : " (turn it on first)"}:</p>
          <div class="url">
            <input type="text" readonly value={withAddress(urls[o.id], live.obs)} aria-label="OBS browser-source URL" onfocus={(e) => e.currentTarget.select()} />
            <Button size="small" onclick={() => copy(o.id)}>{copied === o.id ? "Copied" : "Copy"}</Button>
            <Button variant="ghost" size="small" title="Stops the old URL working" onclick={() => newKey(o.id)}>New key</Button>
          </div>
        {/if}
        {#if !isCalibrated(o)}
          <p class="calibration"><Dot tone="warn" size={6} />Not calibrated: text may be cut off at the edges</p>
        {/if}
      </div>
      <div class="controls">
        <Switch checked={o.enabled} onchange={(on) => putOutput({ ...o, enabled: on })} label={o.enabled ? "On" : "Off"} />
        <Button variant={isCalibrated(o) ? "secondary" : "primary"} size="small" icon="safeArea" onclick={() => oncalibrate(o.id)}>
          {isCalibrated(o) ? "Recalibrate" : "Calibrate"}
        </Button>
        <Button variant="ghost" size="small" icon="trash" onclick={() => removeOutput(o.id)}>Remove</Button>
      </div>
    </li>
  {:else}
    <li class="empty muted">No pictures yet. Add one, then paste its URL into OBS as a Browser source.</li>
  {/each}
</ul>
<div class="add">
  <Button icon="plus" onclick={add}>Add a picture for OBS</Button>
</div>

<style>
  .theme {
    display: inline-flex;
    align-items: center;
    gap: 0.5rem;
    margin-top: 0.5rem;
    font-size: 0.85rem;
  }
  .sources {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    margin-top: 0.75rem;
  }
  .source {
    display: flex;
    justify-content: space-between;
    gap: 1rem;
    padding: 0.9rem 1rem;
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    background: var(--panel);
  }
  .info {
    min-width: 0;
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  .title-row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
  h3 {
    font-size: 0.95rem;
    font-weight: 600;
    margin-right: auto;
  }
  .small {
    font-size: 0.8125rem;
  }
  .url {
    display: flex;
    gap: 0.5rem;
    align-items: center;
  }
  .url input {
    flex: 1;
    min-width: 0;
    font: inherit;
    font-family: ui-monospace, monospace;
    font-size: 0.8rem;
    padding: 0.35rem 0.5rem;
    color: var(--text);
    background: transparent;
    border: 1px solid var(--line);
    border-radius: var(--radius);
  }
  .calibration {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.8125rem;
    color: var(--warn, var(--gold));
  }
  .controls {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 0.5rem;
  }
  .empty {
    padding: 0.75rem 0;
    font-size: 0.875rem;
  }
  .add {
    margin-top: 0.75rem;
  }
</style>
