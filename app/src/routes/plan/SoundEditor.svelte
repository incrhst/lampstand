<!-- Edits one sound in the order of the service: a music-only item or a song's
     backing track. Its name, how it starts (on its own, or when somebody
     presses play), whether it stays off the stream, and whether its file is
     on this device. Everything is a real labelled control; a missing file is
     said in words with an icon, never by colour alone. -->
<script lang="ts">
  import type { Sound, SoundStart } from "../../lib/api";
  import Icon from "../../lib/kit/Icon.svelte";
  import Segmented from "../../lib/kit/Segmented.svelte";
  import { lengthLabel, words } from "../../lib/sound";

  let {
    sound,
    label,
    missing,
    onchange,
  }: {
    sound: Sound;
    /** Names the sound, for the controls' accessible labels. */
    label: string;
    /** Its file is not on this device. */
    missing: boolean;
    onchange: (sound: Sound) => void;
  } = $props();

  let name = $state("");
  $effect(() => {
    name = sound.name;
  });

  const starts: readonly { id: SoundStart; label: string }[] = [
    { id: "OnItsOwn", label: words.onItsOwn },
    { id: "Waits", label: words.waits },
  ];

  function commitName() {
    const next = name.trim();
    if (next === "") {
      name = sound.name;
      return;
    }
    if (next !== sound.name) onchange({ ...sound, name: next });
  }
</script>

<div class="sound-editor">
  <label class="field">
    <span class="eyebrow">{words.nameLabel}</span>
    <input
      type="text"
      aria-label="{words.nameLabel} of {label}"
      bind:value={name}
      onchange={commitName}
    />
  </label>
  <div class="field">
    <span class="eyebrow">{words.startLabel}</span>
    <Segmented
      options={starts}
      value={sound.start}
      label="{words.startLabel}: {label}"
      onchange={(start) => onchange({ ...sound, start })}
    />
  </div>
  <label class="check">
    <input
      type="checkbox"
      checked={sound.keep_off_stream}
      onchange={(e) => onchange({ ...sound, keep_off_stream: e.currentTarget.checked })}
    />
    <span>{words.keepOffStream}</span>
  </label>
  <p class="file muted">
    <span>Length {lengthLabel(sound.length_ms)}</span>
  </p>
  {#if missing}
    <p class="file missing" role="status">
      <Icon name="alert" size={14} />
      <span>{words.missing}</span>
    </p>
  {/if}
</div>

<style>
  .sound-editor {
    display: grid;
    gap: var(--space-2, 8px);
  }
  .field {
    display: grid;
    gap: 4px;
  }
  .check {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.85rem;
  }
  .file {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    margin: 0;
    font-size: 0.8rem;
  }
  .missing {
    color: var(--warn);
    font-weight: 600;
  }
</style>
