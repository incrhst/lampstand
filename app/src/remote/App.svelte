<!-- Lampstand Remote's page: the PIN form until this phone or tablet is
     paired, then the remote: the wall for a phone that runs the service, or
     the speaker's page for a speaker's phone (0.9.1). One revoked on the
     laptop, or a speaker's whose service closed, comes back to the PIN form. -->
<script lang="ts">
  import { onMount } from "svelte";
  import { link } from "./link.svelte";
  import Pair from "./Pair.svelte";
  import Remote from "./Remote.svelte";
  import Speaker from "./Speaker.svelte";
  import type { PairedAs } from "../lib/remote.ts";

  let phase: "checking" | "pair" | "remote" = $state("checking");
  /** Until the socket's hello, what the session check said. */
  let asked: PairedAs | null = $state(null);
  let kind = $derived(link.state === "online" ? link.kind : (asked ?? link.kind));

  /** What the session check says this phone is paired for; null if it is not paired. Throws when the laptop does not answer. */
  async function session(): Promise<PairedAs | null> {
    const r = await fetch("/remote/session", { cache: "no-store" });
    if (r.status === 401) return null;
    const body = (await r.json().catch(() => ({}))) as { kind?: PairedAs };
    return body.kind ?? { kind: "Operator" };
  }

  onMount(async () => {
    try {
      asked = await session();
      phase = asked ? "remote" : "pair";
    } catch {
      // The laptop did not answer: show the remote, which says so and keeps trying.
      phase = "remote";
    }
    if (phase === "remote") link.start();
  });

  $effect(() => {
    if (link.state === "unpaired" || link.state === "revoked") phase = "pair";
  });

  async function paired() {
    asked = await session().catch(() => null);
    phase = "remote";
    link.start();
  }
</script>

{#if phase === "pair"}
  <Pair revoked={link.state === "revoked"} onpaired={paired} />
{:else if phase === "remote" && kind.kind === "Speaker"}
  <Speaker item={kind.item} title={kind.title} />
{:else if phase === "remote"}
  <Remote />
{/if}
