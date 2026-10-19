<!-- A thin progress bar, for imports and per-category migration progress. -->
<script lang="ts">
  let {
    value,
    label,
    tone = "gold",
    width,
  }: {
    /** 0 to 1, or null when the amount done is unknown (the bar sweeps). */
    value: number | null;
    /** Accessible name, e.g. "Importing songs". */
    label: string;
    tone?: "gold" | "ready";
    /** A CSS width; fills its container when left out. */
    width?: string;
  } = $props();
  let percent = $derived(value === null ? null : Math.round(Math.min(1, Math.max(0, value)) * 100));
</script>

<span
  class="track"
  style:width
  role="progressbar"
  aria-label={label}
  aria-valuemin={0}
  aria-valuemax={100}
  aria-valuenow={percent ?? undefined}
>
  <span class="fill {tone}" class:sweep={percent === null} style:width={percent === null ? undefined : `${percent}%`}></span>
</span>

<style>
  .track {
    position: relative;
    display: inline-block;
    height: 4px;
    width: 100%;
    border-radius: 999px;
    overflow: hidden;
    background: var(--line);
    vertical-align: middle;
  }
  .fill {
    position: absolute;
    inset: 0 auto 0 0;
    border-radius: 999px;
    background: var(--gold);
    transition: width 300ms var(--ease);
  }
  .ready {
    background: var(--ready);
  }
  .sweep {
    width: 35%;
    animation: sweep 1.4s var(--ease) infinite;
  }
  @keyframes sweep {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(300%);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .sweep {
      animation: none;
      width: 100%;
      opacity: 0.5;
    }
  }
</style>
