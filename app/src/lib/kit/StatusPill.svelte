<!-- A rounded status label: "Online · everything is on this device". -->
<script lang="ts">
  import type { Snippet } from "svelte";
  import Icon from "./Icon.svelte";
  import type { IconName } from "./icons";

  let {
    tone = "neutral",
    icon,
    children,
  }: { tone?: "ready" | "warn" | "live" | "gold" | "neutral"; icon?: IconName; children: Snippet } = $props();
</script>

<span class="pill {tone}">
  {#if icon}<Icon name={icon} size={14} />{/if}
  {@render children()}
</span>

<style>
  .pill {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    border-radius: 999px;
    padding: 0.2rem 0.65rem;
    font-size: 0.75rem;
    font-weight: 500;
    white-space: nowrap;
    background: var(--raised);
    color: var(--muted);
  }
  .ready {
    background: var(--ready-soft);
    color: var(--ready-text);
  }
  .warn {
    background: var(--warn-soft);
    color: var(--warn);
  }
  .live {
    background: var(--live-strong);
    color: white;
    font-weight: 700;
    letter-spacing: 0.04em;
  }
  .gold {
    background: var(--gold-soft);
    color: var(--gold);
  }
</style>
