<!-- The button for driving the presenting device from this one: "Drive from
     this device" while the presenter is connected, then the state of the
     ask (waiting, not now, someone else is driving). Shows nothing on the
     presenting device or while the presenter is offline. -->
<script lang="ts">
  import Button from "./Button.svelte";
  import { live, presenterRequest } from "../state.svelte";
  import { driveChoice } from "../driving";

  let { size = "normal" }: { size?: "normal" | "small" } = $props();

  let choice = $derived(driveChoice(live.team?.presenter.drive, live.team?.presenter.holder_name ?? ""));
</script>

{#if choice.kind === "ask"}
  <Button icon="monitor" {size} onclick={() => presenterRequest("DriveAsk")} title="Asks the presenting device to let this one operate it. The screens stay there; you press Next here.">
    {choice.label}
  </Button>
{:else if choice.kind === "waiting"}
  <span class="state" role="status">
    <span>{choice.label}</span>
    <Button {size} onclick={() => presenterRequest("DriveCancel")}>Cancel</Button>
  </span>
{:else if choice.kind === "declined" || choice.kind === "busy"}
  <span class="state" role="status">
    <span>{choice.label}</span>
    <Button {size} onclick={() => presenterRequest("DriveAsk")}>{choice.again}</Button>
    <Button variant="ghost" {size} onclick={() => presenterRequest("DriveDismiss")}>OK</Button>
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
</style>
