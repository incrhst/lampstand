<!-- Setup › Sound: whether this device offers music and backing tracks in the
     order of the service. Off by default, so a church that does not use sound
     sees nothing about it in Plan. Sound already in a service always shows. -->
<script lang="ts">
  import { live, saveSettings } from "../../lib/state.svelte";
  import { words } from "../../lib/sound";
  import Panel from "../../lib/kit/Panel.svelte";
  import Switch from "../../lib/kit/Switch.svelte";
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
  .small {
    margin-top: 0.6rem;
    font-size: 0.85rem;
    max-width: 40rem;
  }
</style>
