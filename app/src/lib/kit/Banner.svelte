<!-- A one-line message strip above a list: readiness, a saved notice. -->
<script lang="ts">
  import type { Snippet } from "svelte";
  import Icon from "./Icon.svelte";
  import type { IconName } from "./icons";

  let {
    tone = "neutral",
    icon,
    children,
  }: { tone?: "ready" | "warn" | "neutral"; icon?: IconName; children: Snippet } = $props();
  const defaults = { ready: "checkCircle", warn: "alert", neutral: "info" } as const;
</script>

<div class="banner {tone}" role={tone === "warn" ? "alert" : "status"}>
  <span class="icon"><Icon name={icon ?? defaults[tone]} /></span>
  <div class="body">{@render children()}</div>
</div>

<style>
  .banner {
    display: flex;
    align-items: flex-start;
    gap: 0.75rem;
    border-radius: var(--radius-lg);
    border: 1px solid var(--line);
    background: var(--raised);
    padding: 0.6rem 1rem;
    font-size: 0.875rem;
  }
  .icon {
    margin-top: 0.1rem;
    color: var(--muted);
  }
  .body {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.25rem 0.6rem;
    min-width: 0;
  }
  .ready {
    border-color: color-mix(in srgb, var(--ready) 30%, transparent);
    background: var(--ready-soft);
  }
  .ready .icon {
    color: var(--ready-text);
  }
  .warn {
    border-color: color-mix(in srgb, var(--warn) 30%, transparent);
    background: var(--warn-soft);
  }
  .warn .icon {
    color: var(--warn);
  }
</style>
