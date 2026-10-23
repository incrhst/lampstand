<!-- The button that takes a service live, on a device that may. On a device
     that is only planning it becomes the way to become the presenter:
     "Ask to present" while the presenter is connected, "Present from here"
     when nobody holds the role or the presenter has been offline long
     enough (it says how long, and asks first), and the state of an ask. -->
<script lang="ts">
  import type { IconName } from "./icons";
  import Button from "./Button.svelte";
  import { live, presenterRequest } from "../state.svelte";
  import { presentChoice } from "../presenting";

  let {
    label,
    icon = "play",
    size = "normal",
    disabled = false,
    title,
    onopen,
    present = "Present from here",
  }: {
    /** What the button says on a presenting device: "Open for live". */
    label?: string;
    icon?: IconName;
    size?: "normal" | "small";
    disabled?: boolean;
    title?: string;
    /** Runs on a presenting device. Without it the button shows only when the device cannot present. */
    onopen?: () => void;
    /** What taking over says on a planning device. */
    present?: string;
  } = $props();

  let choice = $derived(presentChoice(live.team?.presenter, present));
  let confirming = $state(false);
  $effect(() => {
    if (choice.kind !== "takeover") confirming = false;
  });

  const claim = () => presenterRequest("PresenterClaim");
  async function takeOver() {
    confirming = false;
    await claim();
  }
</script>

{#if choice.kind === "go"}
  {#if onopen}
    <Button variant="primary" {icon} {size} {disabled} {title} onclick={onopen}>{label}</Button>
  {/if}
{:else if choice.kind === "claim"}
  <Button variant="primary" icon="monitor" {size} onclick={claim} title="Nobody is presenting. This makes this device the one that drives the screens.">{choice.label}</Button>
{:else if choice.kind === "ask"}
  <Button variant="primary" icon="monitor" {size} onclick={() => presenterRequest("PresenterAsk")} title="Asks the presenting device to hand over. It shows a prompt there.">{choice.label}</Button>
{:else if choice.kind === "takeover"}
  {#if confirming}
    <span class="confirm" role="alert">
      <span>{choice.confirm}</span>
      <Button variant="primary" {size} onclick={takeOver}>Present from here</Button>
      <Button {size} onclick={() => (confirming = false)}>Not now</Button>
    </span>
  {:else}
    <Button variant="primary" icon="monitor" {size} onclick={() => (confirming = true)}>{choice.label}</Button>
  {/if}
{:else if choice.kind === "waiting"}
  <span class="state" role="status">
    <span>{choice.label}</span>
    <Button {size} onclick={() => presenterRequest("PresenterCancel")}>Cancel</Button>
  </span>
{:else if choice.kind === "declined"}
  <span class="state" role="status">
    <span>{choice.label}</span>
    <Button {size} onclick={() => presenterRequest("PresenterAsk")}>{choice.again}</Button>
    <Button variant="ghost" {size} onclick={() => presenterRequest("PresenterCancel")}>OK</Button>
  </span>
{:else}
  <span class="state">
    <Button {size} disabled title={choice.hint} onclick={() => {}}>{choice.label}</Button>
    <small class="muted">{choice.hint}</small>
  </span>
{/if}

<style>
  .confirm,
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
