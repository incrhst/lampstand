<!-- A small status dot. Always next to words that say the same thing. -->
<script lang="ts">
  let { tone = "neutral", size = 6 }: { tone?: "ready" | "live" | "warn" | "gold" | "neutral"; size?: number } =
    $props();
</script>

<span class="dot {tone}" style:width="{size}px" style:height="{size}px" aria-hidden="true"></span>

<style>
  .dot {
    display: inline-block;
    flex-shrink: 0;
    border-radius: 50%;
    background: var(--faint);
  }
  .ready {
    background: var(--ready);
  }
  .live {
    background: var(--live);
  }
  .warn {
    background: var(--warn);
  }
  .gold {
    background: var(--gold);
  }
</style>
