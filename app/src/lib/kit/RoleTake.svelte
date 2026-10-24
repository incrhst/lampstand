<!-- The "Take" button for one role: a click where nobody holds it, an ask
     while the holder is connected, a confirmed takeover once the holder has
     been offline for the takeover time, and a greyed button that says why
     when it cannot be taken. Shows nothing for a role this device holds. -->
<script lang="ts">
  import Button from "./Button.svelte";
  import { live, presenterRequest } from "../state.svelte";
  import type { RoleName } from "../api";
  import { NOT_NOW, roleChoice, roleOf, takeName } from "../roles";

  let { role, size = "normal" }: { role: RoleName; size?: "normal" | "small" } = $props();

  let view = $derived(roleOf(live.team, role));
  let choice = $derived(roleChoice(view));
  let confirming = $state(false);
  $effect(() => {
    if (choice.kind !== "takeover") confirming = false;
  });
  const claim = () => presenterRequest({ RoleClaim: { role } });
  async function takeOver() {
    confirming = false;
    await claim();
  }
</script>

{#if choice.kind === "claim"}
  <Button variant="primary" {size} onclick={claim} label={takeName(role)}>{choice.label}</Button>
{:else if choice.kind === "ask"}
  <Button variant="primary" {size} onclick={() => presenterRequest({ RoleAsk: { role } })} label={takeName(role)}>{choice.label}</Button>
{:else if choice.kind === "takeover"}
  {#if confirming}
    <span class="state" role="alert">
      <span>{choice.confirm}</span>
      <Button variant="primary" {size} onclick={takeOver} label={takeName(role)}>{choice.ok}</Button>
      <Button {size} onclick={() => (confirming = false)}>{NOT_NOW}</Button>
    </span>
  {:else}
    <Button variant="primary" {size} onclick={() => (confirming = true)} label={takeName(role)}>{choice.label}</Button>
  {/if}
{:else if choice.kind === "waiting"}
  <span class="state" role="status">
    <span>{choice.label}</span>
    <Button {size} onclick={() => presenterRequest({ RoleCancel: { role } })}>Cancel</Button>
  </span>
{:else if choice.kind === "declined" || choice.kind === "busy"}
  <span class="state" role="status">
    <span>{choice.label}</span>
    <Button {size} onclick={() => presenterRequest({ RoleAsk: { role } })}>{choice.again}</Button>
    <Button variant="ghost" {size} onclick={() => presenterRequest({ RoleCancel: { role } })}>OK</Button>
  </span>
{:else if choice.kind === "blocked" || choice.kind === "wait"}
  <span class="state">
    <Button {size} disabled title={choice.hint} label={takeName(role)} onclick={() => {}}>{choice.label}</Button>
    <small class="muted">{choice.hint}</small>
  </span>
{/if}

<style>
  .state {
    display: inline-flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.875rem;
  }
  .muted {
    color: var(--muted);
  }
</style>
