<!-- The loop of the section on air, changed without leaving the operator
     view: on or off, and how long each still slide stays up. A change takes
     effect from the next slide and belongs to this service's run only; the
     saved section is changed in Plan. -->
<script lang="ts">
  import { request, type ItemEstimate, type LoadedService, type SectionView } from "../../lib/api";
  import { report } from "../../lib/state.svelte";
  import { estimatePass, passLabel } from "../../lib/loops";

  let { loaded, section }: { loaded: LoadedService; section: SectionView } = $props();

  const choices = [5, 8, 12, 20];
  let estimates: Record<string, ItemEstimate> = $state({});
  /** What the loop was set to before it was turned off, to turn it back on. */
  let lastSeconds = $state(8);

  $effect(() => {
    if (section.seconds) lastSeconds = section.seconds;
  });

  $effect(() => {
    const service = loaded.service;
    request({ area: "Service", request: { Estimate: service } }, "Estimate")
      .then((list) => (estimates = Object.fromEntries(list.map((e) => [e.item, e]))))
      .catch(() => {});
  });

  let rows = $derived(
    section.items.map((id) => ({ id, header: false, title: loaded.items.find((i) => i.id === id)?.title ?? "" })),
  );
  let pass = $derived(section.seconds ? estimatePass(rows, estimates, section.seconds) : null);

  async function set(seconds: number | null) {
    try {
      await request({ area: "Service", request: { SetLiveLoop: { header: section.header, seconds } } }, "Ok");
    } catch (e) {
      report(e);
    }
  }
</script>

<section class="live-loop" aria-label="Loop {section.title}">
  <div class="head">
    <span class="name">{section.title}</span>
  </div>
  <div class="segments" class:off={section.seconds === null} role="group" aria-label="Seconds per still slide">
    {#each choices as n (n)}
      <button type="button" class="seg" class:on={section.seconds === n} aria-pressed={section.seconds === n} onclick={() => set(n)}>{n} s</button>
    {/each}
  </div>
  {#if pass}
    <p class="pass"><span class="muted">One pass</span> <strong>{passLabel(pass)}</strong></p>
  {/if}
  <p class="muted note">
    {section.seconds === null ? "Looping is off. The section stops on its last slide." : "Videos play through. Changes apply from the next slide."}
  </p>
</section>

<style>
  .live-loop {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    margin: 0 0.75rem 0.5rem;
    padding: 0.75rem;
    border: 1px solid var(--gold);
    border-radius: var(--radius-lg);
    background: var(--panel);
    font-size: 0.8125rem;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
  }
  .name {
    font-weight: 600;
  }
  .segments {
    display: flex;
    flex-wrap: wrap;
    gap: 0.3rem;
  }
  .segments.off {
    opacity: 0.5;
  }
  .seg {
    font: inherit;
    padding: 0.25rem 0.6rem;
    border-radius: var(--radius);
    border: 1px solid var(--line-strong);
    background: var(--raised);
    color: var(--text);
    cursor: pointer;
  }
  .seg.on {
    background: var(--gold);
    border-color: var(--gold);
    color: var(--on-gold);
    font-weight: 600;
  }
  .pass strong {
    color: var(--gold);
  }
  .note {
    font-size: 0.75rem;
    line-height: 1.4;
  }
</style>
