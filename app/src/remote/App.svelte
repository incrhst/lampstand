<!-- Lampstand Remote's page: the PIN form until this phone or tablet is
     paired, then the remote. One revoked on the laptop comes back to the PIN
     form. -->
<script lang="ts">
  import { onMount } from "svelte";
  import { link } from "./link.svelte";
  import Pair from "./Pair.svelte";
  import Remote from "./Remote.svelte";

  let phase: "checking" | "pair" | "remote" = $state("checking");

  onMount(async () => {
    try {
      const r = await fetch("/remote/session", { cache: "no-store" });
      phase = r.status === 401 ? "pair" : "remote";
    } catch {
      // The laptop did not answer: show the remote, which says so and keeps trying.
      phase = "remote";
    }
    if (phase === "remote") link.start();
  });

  $effect(() => {
    if (link.state === "unpaired" || link.state === "revoked") phase = "pair";
  });

  function paired() {
    phase = "remote";
    link.start();
  }
</script>

{#if phase === "pair"}
  <Pair revoked={link.state === "revoked"} onpaired={paired} />
{:else if phase === "remote"}
  <Remote />
{/if}
