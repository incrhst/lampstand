<!-- Setup › About: the version running, and updating it in place. A new
     release is found by asking the release page (the core does this, at
     startup and every few hours), and installed from here when nothing is on
     air and no service is about to start, so an update never interrupts one. -->
<script lang="ts">
  import { checkForUpdate, installUpdate, live, refreshUpdate } from "../../lib/state.svelte";
  import { availableVersion, canInstall, updateSummary } from "../../lib/updates";
  import Button from "../../lib/kit/Button.svelte";
  import Panel from "../../lib/kit/Panel.svelte";
  import StatusPill from "../../lib/kit/StatusPill.svelte";

  let checking = $state(false);

  // Know the version and what the last check found as the page opens.
  refreshUpdate();

  async function check() {
    checking = true;
    await checkForUpdate(false);
    checking = false;
  }

  let status = $derived(live.update);
  let summary = $derived(updateSummary(status));
  let version = $derived(status ? availableVersion(status.state) : null);
  let notes = $derived(status && typeof status.state === "object" && "Available" in status.state ? status.state.Available.notes : null);
  let installing = $derived(status?.state === "Installing");
</script>

<div class="page">
  <h1>About Lampstand</h1>
  <p class="muted intro">
    Free and open-source church projection. Version <strong>{status?.current ?? "…"}</strong>.
  </p>

  <Panel title="Updates" description="Lampstand looks for a new version when it starts and every few hours. It installs one only when you choose to.">
    {#snippet actions()}
      <StatusPill tone={summary.tone}>{summary.text}</StatusPill>
    {/snippet}

    {#if version}
      {#if notes}<pre class="notes">{notes}</pre>{/if}
      <div class="buttons">
        <Button variant="primary" onclick={installUpdate} disabled={!canInstall(status) || installing}>
          {installing ? "Installing…" : `Install ${version} and restart`}
        </Button>
        <Button variant="ghost" onclick={check} disabled={checking || installing}>Check again</Button>
      </div>
      {#if status?.blocked}<p class="warn">{status.blocked}</p>{/if}
      <p class="muted small">Your services, songs and settings stay as they are. The app closes and opens again when it is installed.</p>
    {:else}
      <div class="buttons">
        <Button variant="primary" onclick={check} disabled={checking}>{checking ? "Checking…" : "Check for updates"}</Button>
      </div>
    {/if}
  </Panel>
</div>

<style>
  .page {
    max-width: 64rem;
    margin: 0 auto;
    padding: 2rem;
  }
  h1 {
    font-size: 1.3rem;
    font-weight: 600;
    letter-spacing: -0.01em;
  }
  .intro {
    margin: 0.3rem 0 1.5rem;
    font-size: 0.875rem;
    line-height: 1.6;
  }
  .buttons {
    display: flex;
    gap: 0.5rem;
    margin-top: 0.75rem;
  }
  .notes {
    margin: 0.75rem 0 0;
    padding: 0.75rem 1rem;
    max-height: 14rem;
    overflow-y: auto;
    white-space: pre-wrap;
    font: inherit;
    font-size: 0.85rem;
    line-height: 1.55;
    border: 1px solid var(--line);
    border-radius: var(--radius);
  }
  .warn {
    margin-top: 0.75rem;
    font-size: 0.875rem;
    color: var(--gold);
  }
  .small {
    margin-top: 0.75rem;
    font-size: 0.8125rem;
  }
</style>
