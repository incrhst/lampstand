<!-- Setup › Sound: whether this device offers music and backing tracks in the
     order of the service. Off by default, so a church that does not use sound
     sees nothing about it in Plan. Sound already in a service always shows. -->
<script lang="ts">
  import { onMount } from "svelte";
  import { request } from "../../lib/api";
  import type { SoundOutputs } from "../../lib/api";
  import { live, report, saveSettings } from "../../lib/state.svelte";
  import { words } from "../../lib/sound";
  import { missingOutput, selectedOutput, words as audio } from "../../lib/audio";
  import Panel from "../../lib/kit/Panel.svelte";
  import Switch from "../../lib/kit/Switch.svelte";

  // The outputs this computer has, asked for when the screen opens.
  let outputs = $state<SoundOutputs | null>(null);
  onMount(() => {
    request({ area: "Sound", request: "Outputs" }, "SoundOutputs")
      .then((found) => (outputs = found))
      .catch(report);
  });
  let chosen = $derived(live.settings?.sound_output ?? null);
  let gone = $derived(outputs ? missingOutput(chosen, outputs.devices) : null);
</script>

<div class="page">
  <h1>{words.setupTitle}</h1>
  <Panel title={words.setupPanel}>
    <Switch
      checked={live.settings?.use_sound ?? false}
      label={words.setupSwitch}
      disabled={live.settings === null}
      onchange={(on) => saveSettings({ use_sound: on })}
    />
    <p class="muted small">{words.setupSentence}</p>
  </Panel>
  <Panel title={audio.outputPanel}>
    {#if outputs && outputs.devices.length === 0}
      <p class="muted small">{audio.outputNone}</p>
    {:else}
      <label class="output">
        {audio.outputLabel}
        <select
          disabled={live.settings === null || outputs === null}
          value={outputs ? selectedOutput(chosen, outputs.devices) : ""}
          onchange={(e) => saveSettings({ sound_output: e.currentTarget.value === "" ? null : e.currentTarget.value })}
        >
          <option value="">{audio.outputDefault(outputs?.default ?? null)}</option>
          {#each outputs?.devices ?? [] as name}
            <option value={name}>{name}</option>
          {/each}
        </select>
      </label>
    {/if}
    {#if gone}<p class="small warn">{audio.outputMissing(gone)}</p>{/if}
    <p class="muted small">{audio.outputSentence}</p>
  </Panel>
</div>

<style>
  .page {
    max-width: 64rem;
    margin: 0 auto;
    padding: 2rem;
  }
  h1 {
    font-size: 1.3rem;
    font-weight: 600;
    letter-spacing: -0.01em;
    margin-bottom: 1rem;
  }
  .output {
    display: grid;
    gap: 0.3rem;
    max-width: 28rem;
  }
  select {
    font: inherit;
    padding: 0.4rem 0.6rem;
    border-radius: 6px;
    border: 1px solid var(--line);
    background: var(--raised);
    color: var(--text);
  }
  .warn {
    color: var(--warn);
  }
  .small {
    margin-top: 0.6rem;
    font-size: 0.85rem;
    max-width: 40rem;
  }
</style>
