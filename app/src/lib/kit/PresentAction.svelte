<!-- The button that takes a service live, on a device that may. On a device
     that is only planning it becomes the way to run the screens from here:
     the Take button for the Projection role, which asks while the holder is
     connected, takes over once it has been offline long enough (it says how
     long, and asks first), and says why when it cannot. -->
<script lang="ts">
  import type { IconName } from "./icons";
  import Button from "./Button.svelte";
  import RoleTake from "./RoleTake.svelte";
  import { live } from "../state.svelte";
  import { roleOf, runFromHere } from "../roles";

  let {
    label,
    icon = "play",
    size = "normal",
    disabled = false,
    title,
    onopen,
  }: {
    /** What the button says on a presenting device: "Open for live". */
    label?: string;
    icon?: IconName;
    size?: "normal" | "small";
    disabled?: boolean;
    title?: string;
    /** Runs on a presenting device. Without it the button shows only when the device cannot present. */
    onopen?: () => void;
  } = $props();

  /** This device runs the screens, or has nobody to share them with. */
  let presenting = $derived(!live.team || live.team.presenter.role === "Presenting" || !roleOf(live.team, "Projection"));
</script>

{#if presenting}
  {#if onopen}
    <Button variant="primary" {icon} {size} {disabled} {title} onclick={onopen}>{label}</Button>
  {/if}
{:else}
  <RoleTake role="Projection" {size} label={runFromHere("Projection")} />
{/if}
