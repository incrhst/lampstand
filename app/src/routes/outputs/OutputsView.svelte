<!-- Bare outputs panel: add a window or display output, enable it.
     T50 replaces this with outputs setup and the calibration wizard. -->
<script lang="ts">
  import { request, type DisplayMatch, type OutputConfig } from "../../lib/api";
  import { report } from "../../lib/state.svelte";
  import Button from "../../lib/kit/Button.svelte";
  import Panel from "../../lib/kit/Panel.svelte";

  let outputs: OutputConfig[] = $state([]);
  let displays: DisplayMatch[] = $state([]);

  async function refresh() {
    try {
      outputs = await request({ area: "Outputs", request: "List" }, "Outputs");
      displays = await request({ area: "Outputs", request: "ListDisplays" }, "Displays");
    } catch (e) {
      report(e);
    }
  }

  async function put(config: OutputConfig) {
    try {
      outputs = await request({ area: "Outputs", request: { Put: config } }, "Outputs");
    } catch (e) {
      report(e);
    }
  }

  async function remove(id: string) {
    try {
      outputs = await request({ area: "Outputs", request: { Remove: id } }, "Outputs");
    } catch (e) {
      report(e);
    }
  }

  function add(name: string, target: OutputConfig["target"]) {
    const primary = !outputs.some((o) => typeof o.role === "object" && o.role.Audience.primary);
    put({
      id: crypto.randomUUID(),
      name,
      role: { Audience: { primary } },
      target,
      safe_area: { top: 0, right: 0, bottom: 0, left: 0 },
      background: "Bleed",
      theme: null,
      enabled: false,
    });
  }

  function describe(o: OutputConfig): string {
    const t = o.target;
    if ("Window" in t) return `Window ${t.Window.width}×${t.Window.height}`;
    if ("Display" in t) return `Fullscreen on ${t.Display.name || "display"} (${t.Display.width}×${t.Display.height})`;
    return `Browser source ${t.BrowserSource.width}×${t.BrowserSource.height}`;
  }

  refresh();
</script>

<div class="grid">
  <Panel title="Outputs">
    {#snippet actions()}
      <Button variant="primary" onclick={() => add("Rehearsal window", { Window: { width: 960, height: 540 } })}>
        Add window output
      </Button>
    {/snippet}
    <ul class="list">
      {#each outputs as o (o.id)}
        <li>
          <span>
            <strong>{o.name}</strong><br />
            <small class="muted">{describe(o)}</small>
          </span>
          <span class="row">
            <label>
              <input
                type="checkbox"
                checked={o.enabled}
                onchange={(e) => put({ ...o, enabled: e.currentTarget.checked })}
              />
              Enabled
            </label>
            <Button onclick={() => remove(o.id)}>Remove</Button>
          </span>
        </li>
      {:else}
        <li class="muted">No outputs. Add a window output to see slides.</li>
      {/each}
    </ul>
  </Panel>

  <Panel title="Displays">
    {#snippet actions()}
      <Button onclick={refresh}>Refresh</Button>
    {/snippet}
    <ul class="list">
      {#each displays as d, i (i)}
        <li>
          <span>{d.name || `Display ${i + 1}`} <small class="muted">{d.width}×{d.height} at {d.x},{d.y}</small></span>
          <Button onclick={() => add(d.name || `Display ${i + 1}`, { Display: d })}>Add fullscreen output</Button>
        </li>
      {/each}
    </ul>
    <p class="muted">OBS browser-source outputs arrive with the local server (T42, T43).</p>
  </Panel>
</div>

<style>
  .grid {
    display: grid;
    grid-template-columns: 1.3fr 1fr;
    gap: 1rem;
    align-items: start;
  }
  .row {
    display: flex;
    gap: 0.6rem;
    align-items: center;
  }
</style>
