<!-- Plan | Present in a Focus pill: the menu bar is hidden in Focus, so the
     way to the other mode stays here. Switching keeps Focus on. -->
<script lang="ts">
  let {
    current,
    mod,
    onplan,
    onpresent,
  }: { current: "plan" | "operate"; mod: string; onplan?: () => void; onpresent?: () => void } = $props();
</script>

<nav class="modes" aria-label="Mode">
  <button type="button" class:active={current === "plan"} aria-current={current === "plan" ? "page" : undefined} title="Plan, still in Focus ({mod}1)" onclick={() => current !== "plan" && onplan?.()}>Plan</button>
  <button type="button" class:active={current === "operate"} aria-current={current === "operate" ? "page" : undefined} title="Present, still in Focus ({mod}2)" onclick={() => current !== "operate" && onpresent?.()}>Present</button>
</nav>

<style>
  .modes {
    display: flex;
    flex-shrink: 0;
    gap: 2px;
    padding: 2px;
    border-radius: 999px;
    background: var(--canvas);
  }
  button {
    font: inherit;
    font-size: 0.8rem;
    min-height: 1.75rem;
    padding: 0 0.75rem;
    border: none;
    border-radius: 999px;
    background: none;
    color: var(--muted);
    cursor: pointer;
  }
  button:hover {
    color: var(--text);
  }
  button.active {
    background: var(--raised);
    color: var(--text);
    cursor: default;
  }
</style>
