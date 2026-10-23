<!-- A dismissible "Did you know?" card for one tip, with a way to the rest. -->
<script lang="ts">
  import Button from "./Button.svelte";
  import Icon from "./Icon.svelte";
  import type { Tip } from "../tips";

  let { tip, ondismiss, onall }: { tip: Tip; ondismiss: () => void; onall: () => void } = $props();
</script>

<aside class="tip" aria-label="Did you know?">
  <span class="eyebrow"><Icon name="lamp" size={14} />Did you know?</span>
  <strong>{tip.title}</strong>
  <p>{tip.body}</p>
  <div class="actions">
    <Button size="small" onclick={ondismiss}>Got it</Button>
    <button type="button" class="link" onclick={onall}>All quick tips</button>
  </div>
</aside>

<style>
  .tip {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    margin: 0 0.75rem 0.5rem;
    padding: 0.75rem;
    border: 1px dashed var(--line-strong);
    border-radius: var(--radius-lg);
    font-size: 0.8125rem;
  }
  .eyebrow {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    font-size: 0.7rem;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--muted);
  }
  p {
    margin: 0;
    color: var(--muted);
    line-height: 1.4;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    margin-top: 0.25rem;
  }
  .link {
    font: inherit;
    background: none;
    border: none;
    padding: 0;
    color: var(--text);
    text-decoration: underline;
    cursor: pointer;
  }
</style>
