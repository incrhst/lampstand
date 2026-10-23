<!-- Messages about who presents that need an answer or should not be
     missed, shown across the top of every screen: another device asking
     this one (the presenter) to hand over, this device's own ask, and
     being replaced as presenter while it was away. -->
<script lang="ts">
  import Banner from "./Banner.svelte";
  import Button from "./Button.svelte";
  import { live, presenterRequest } from "../state.svelte";
  import { replacedNotice, requestPrompt } from "../presenting";

  let p = $derived(live.team?.presenter ?? null);
  /** Something is on air here, so allowing takes it off. */
  let onAir = $derived(!!live.live || live.overlay !== "None" || (!!live.loaded && !live.idle));
  let allowed = $state(false);
  let wasAsking = false;
  // An ask that ends with this device presenting: say so once.
  $effect(() => {
    const asking = !!p?.asking && !p.asking.declined;
    if (wasAsking && !asking && p?.role === "Presenting") {
      allowed = true;
      setTimeout(() => (allowed = false), 8000);
    }
    wasAsking = asking;
  });
</script>

{#if p && (p.requests.length > 0 || p.replaced_by || allowed || (p.asking && p.role === "Planning"))}
  <div class="notices">
    {#each p.requests as r (r.device)}
      <Banner tone="warn" icon="monitor">
        <span>{requestPrompt(r.name, onAir)}</span>
        <Button variant="primary" size="small" onclick={() => presenterRequest({ PresenterAllow: { device: r.device } })}>
          {onAir ? "Allow and clear the screens" : "Allow"}
        </Button>
        <Button size="small" onclick={() => presenterRequest({ PresenterDecline: { device: r.device } })}>Not now</Button>
      </Banner>
    {/each}
    {#if p.replaced_by}
      <Banner tone="warn">
        <span>{replacedNotice(p.replaced_by)}</span>
        <Button size="small" onclick={() => presenterRequest("PresenterDismiss")}>OK</Button>
      </Banner>
    {/if}
    {#if p.asking && p.role === "Planning"}
      <Banner tone="neutral" icon="monitor">
        {#if p.asking.declined}
          <span>{p.holder_name} said not now.</span>
          <Button size="small" onclick={() => presenterRequest("PresenterAsk")}>Ask again</Button>
          <Button variant="ghost" size="small" onclick={() => presenterRequest("PresenterCancel")}>OK</Button>
        {:else}
          <span>Waiting for {p.holder_name} to allow this device to present…</span>
          <Button size="small" onclick={() => presenterRequest("PresenterCancel")}>Cancel</Button>
        {/if}
      </Banner>
    {/if}
    {#if allowed}
      <Banner tone="ready">This device is presenting now. Its screens are yours.</Banner>
    {/if}
  </div>
{/if}

<style>
  .notices {
    display: grid;
    gap: 0.4rem;
    padding: 0.5rem 1.25rem 0;
  }
</style>
