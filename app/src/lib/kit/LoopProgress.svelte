<!-- Time left on the slide while a loop is running: a thin bar along the
     bottom edge of its container that fills over `seconds` and starts again
     whenever `slideKey` changes (a new slide went up). The container must be
     positioned. -->
<script lang="ts">
  let { seconds, slideKey }: { seconds: number; slideKey: string } = $props();
</script>

{#key slideKey}
  <span class="loop-progress" aria-hidden="true">
    <span class="fill" style:animation-duration="{seconds}s"></span>
  </span>
{/key}

<style>
  .loop-progress {
    position: absolute;
    inset: auto 0 0 0;
    height: 4px;
    background: rgba(0, 0, 0, 0.35);
    pointer-events: none;
  }
  .fill {
    display: block;
    height: 100%;
    background: var(--gold);
    transform-origin: left;
    animation: fill linear forwards;
  }
  @keyframes fill {
    from {
      transform: scaleX(0);
    }
    to {
      transform: scaleX(1);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .fill {
      animation: none;
      transform: scaleX(0.5);
      opacity: 0.5;
    }
  }
</style>
