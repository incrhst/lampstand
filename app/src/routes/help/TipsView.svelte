<!-- Every quick tip, in one place: the "Did you know?" cards that were
     dismissed are still here. -->
<script lang="ts">
  import { TIPS } from "../../lib/tips";

  const reduced = globalThis.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;
</script>

<div class="tips">
  <h2>Quick tips</h2>
  <p class="muted">Short notes on what Lampstand can do. A tip also shows up as a card in Present when it applies.</p>
  <ul class="plain">
    {#each TIPS as tip (tip.id)}
      <li>
        {#if tip.clip}
          <!-- Muted, looping and silent: a picture that moves. Reduced motion gets the first frame. -->
          <video class="clip" src={tip.clip} muted loop playsinline preload="metadata" autoplay={!reduced} aria-hidden="true"></video>
        {/if}
        <span class="area">{tip.area}</span>
        <strong>{tip.title}</strong>
        <p class="muted">{tip.body}</p>
      </li>
    {/each}
  </ul>
</div>

<style>
  .tips {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    padding: 1rem 1.25rem;
    max-width: 40rem;
  }
  h2 {
    margin: 0;
  }
  ul {
    display: flex;
    flex-direction: column;
    gap: 1rem;
    margin-top: 0.5rem;
  }
  li {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
  }
  li p {
    margin: 0;
  }
  .clip {
    width: 100%;
    max-width: 30rem;
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    background: var(--panel);
  }
  .area {
    font-size: 0.7rem;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--faint);
  }
</style>
