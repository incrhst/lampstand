<!-- A small gold "Start here" label with a slow pulse around it, beside the
     control a new church should press first. Shown only until that step is
     done, so it never becomes noise. With reduced motion it keeps the
     colour and drops the pulse. -->
<script lang="ts">
  let { label = "Start here" }: { label?: string } = $props();
</script>

<span class="pulse-badge"><span class="dot" aria-hidden="true"></span>{label}</span>

<style>
  .pulse-badge {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.1rem 0.5rem 0.1rem 0.4rem;
    border-radius: 999px;
    background: var(--gold-soft);
    border: 1px solid var(--gold);
    color: var(--gold);
    font-size: 0.75rem;
    font-weight: 600;
    white-space: nowrap;
  }
  .dot {
    position: relative;
    width: 0.5rem;
    height: 0.5rem;
    border-radius: 50%;
    background: var(--gold);
  }
  .dot::after {
    content: "";
    position: absolute;
    inset: 0;
    border-radius: 50%;
    border: 2px solid var(--gold);
    animation: pulse 1.6s ease-out infinite;
  }
  @keyframes pulse {
    from {
      transform: scale(1);
      opacity: 0.9;
    }
    to {
      transform: scale(2.8);
      opacity: 0;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .dot::after {
      animation: none;
      opacity: 0;
    }
  }
</style>
