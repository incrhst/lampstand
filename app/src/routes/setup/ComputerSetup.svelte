<!-- Setup › This computer: the two switches that reach past Lampstand's own
     windows. Both are off by default and belong to this computer, so they are
     not shared with the church's other devices. -->
<script lang="ts">
  import { live, saveSettings } from "../../lib/state.svelte";
  import { words } from "../../lib/computer";
  import Panel from "../../lib/kit/Panel.svelte";
  import Switch from "../../lib/kit/Switch.svelte";

  // The computer can refuse the startup item (the status bar says why). The
  // saved setting is then unchanged, so draw the switch again to match it.
  let again = $state(0);
  async function startWhenComputerStarts(on: boolean) {
    await saveSettings({ start_at_login: on });
    if ((live.settings?.start_at_login ?? false) !== on) again++;
  }
</script>

<div class="page">
  <h1>{words.title}</h1>
  <Panel title={words.startPanel}>
    {#key again}
      <Switch
        checked={live.settings?.start_at_login ?? false}
        label={words.startSwitch}
        disabled={live.settings === null}
        onchange={startWhenComputerStarts}
      />
    {/key}
    <p class="muted small">{words.startSentence}</p>
  </Panel>
  <Panel title={words.awakePanel}>
    <Switch
      checked={live.settings?.keep_awake ?? false}
      label={words.awakeSwitch}
      disabled={live.settings === null}
      onchange={(on) => saveSettings({ keep_awake: on })}
    />
    <p class="muted small">{words.awakeSentence}</p>
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
