<!-- The "Take" button for one role: a click where nobody holds it, an ask
     while the holder is connected, a confirmed takeover once the holder has
     been offline for the takeover time, and a greyed button that says why
     when it cannot be taken. Shows nothing for a role this device holds.
     While an ask is open the button is greyed; the single status message
     (with Cancel, Ask again) is RolePrompts', across the top of the screen. -->
<script lang="ts">
  import { tick } from "svelte";
  import Button from "./Button.svelte";
  import { live, presenterRequest } from "../state.svelte";
  import type { RoleName } from "../api";
  import { NOT_NOW, roleChoice, roleOf, takeName } from "../roles";

  let {
    role,
    size = "normal",
    label,
  }: {
    role: RoleName;
    size?: "normal" | "small";
    /** What the button says where it stands alone ("Run the service from here"); "Take" in a list of roles. */
    label?: string;
  } = $props();

  let view = $derived(roleOf(live.team, role));
  let choice = $derived(roleChoice(view));
  let confirming = $state(false);
  let box: HTMLElement | undefined = $state();
  const uid = Math.random().toString(36).slice(2, 8);
  const hintId = $derived(`take-hint-${role}-${uid}`);
  const name = $derived(takeName(role, label));
  $effect(() => {
    if (choice.kind !== "takeover") confirming = false;
  });
  const claim = () => presenterRequest({ RoleClaim: { role } });
  async function takeOver() {
    confirming = false;
    await claim();
  }
  /** Moves focus to the first button in this control once it has been drawn. */
  async function focusFirst() {
    await tick();
    box?.querySelector("button")?.focus();
  }
  async function askToConfirm() {
    confirming = true;
    await focusFirst();
  }
  async function cancelConfirm() {
    confirming = false;
    await focusFirst();
  }
</script>

<span class="box" bind:this={box}>
  {#if choice.kind === "claim"}
    <Button variant="primary" {size} onclick={claim} label={name}>{label ?? choice.label}</Button>
  {:else if choice.kind === "ask"}
    <Button variant="primary" {size} onclick={() => presenterRequest({ RoleAsk: { role } })} label={name}>{label ?? choice.label}</Button>
  {:else if choice.kind === "takeover"}
    {#if confirming}
      <span class="state" role="alert">
        <span>{choice.confirm}</span>
        <Button variant="primary" {size} onclick={takeOver} label={takeName(role)}>{choice.ok}</Button>
        <Button {size} onclick={cancelConfirm}>{NOT_NOW}</Button>
      </span>
    {:else}
      <Button variant="primary" {size} onclick={askToConfirm} label={name}>{label ?? choice.label}</Button>
    {/if}
  {:else if choice.kind === "waiting" || choice.kind === "declined" || choice.kind === "busy"}
    <span class="state">
      <Button {size} disabled describedby={hintId} label={name} onclick={() => {}}>{label ?? "Take"}</Button>
      <small class="muted" id={hintId}>{choice.label}</small>
    </span>
  {:else if choice.kind === "blocked" || choice.kind === "wait"}
    <span class="state">
      <Button {size} disabled describedby={hintId} label={name} onclick={() => {}}>{label ?? choice.label}</Button>
      <small class="muted" id={hintId}>{choice.hint}</small>
    </span>
  {/if}
</span>

<style>
  .box {
    display: inline-flex;
  }
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
