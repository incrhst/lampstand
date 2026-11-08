<!-- An on/off switch with its label, e.g. enabling an output. -->
<script lang="ts">
  let {
    checked,
    onchange,
    label,
    disabled = false,
  }: { checked: boolean; onchange: (on: boolean) => void; label: string; disabled?: boolean } = $props();
</script>

<label class="switch" class:disabled>
  <input type="checkbox" role="switch" {checked} {disabled} onchange={(e) => onchange(e.currentTarget.checked)} />
  <span class="track" aria-hidden="true"><span class="thumb"></span></span>
  <span>{label}</span>
</label>

<style>
  .switch {
    display: inline-flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.85rem;
    cursor: pointer;
    position: relative;
  }
  .switch.disabled {
    cursor: default;
    opacity: 0.6;
  }
  input {
    position: absolute;
    opacity: 0;
    width: 1px;
    height: 1px;
  }
  .track {
    width: 2rem;
    height: 1.1rem;
    border-radius: 999px;
    background: var(--line-strong);
    position: relative;
  }
  .thumb {
    position: absolute;
    top: 2px;
    left: 2px;
    width: calc(1.1rem - 4px);
    height: calc(1.1rem - 4px);
    border-radius: 50%;
    background: var(--text);
  }
  input:checked + .track {
    background: var(--ready);
  }
  input:checked + .track .thumb {
    transform: translateX(0.9rem);
  }
  input:focus-visible + .track {
    outline: 2px solid var(--gold);
    outline-offset: 2px;
  }
</style>
