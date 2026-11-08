<!-- The bar that says who runs the service from whose screens, across the top
     of every screen. On a device that runs the service for another's
     screens: who runs the screens (or that they are not connected, so the
     service controls wait) with a button to hand the service back. On a
     device that does not: a Take button for the service. On the device that
     runs the screens while another runs the service: who, with Take, which
     always works there. -->
<script lang="ts">
  import Banner from "./Banner.svelte";
  import Button from "./Button.svelte";
  import { live, presenterRequest } from "../state.svelte";
  import RoleTake from "./RoleTake.svelte";
  import { HAND_BACK, runFromHere, runsServiceBar, screensBar, serviceFromHereBar } from "../roles";
  import { drivenBy, drivingFrom } from "../driving";

  let driving = $derived(drivingFrom(live.team));
  let by = $derived(drivenBy(live.team));
  /** A device that does not run the screens, with a connected one that does, or an ask of its own to show: always one click from running the service. */
  let offer = $derived(
    !!live.team && live.team.presenter.role === "Planning" && (live.team.presenter.drive.can_drive || !!live.team.presenter.drive.asking),
  );
</script>

{#if driving}
  <Banner tone={driving.online ? "ready" : "warn"} icon="monitor">
    <span>{serviceFromHereBar(driving.name, driving.online)}</span>
    <Button size="small" onclick={() => presenterRequest("DriveCancel")}>{HAND_BACK}</Button>
  </Banner>
{:else if offer}
  <Banner tone="neutral" icon="monitor">
    <span>{screensBar(live.team?.presenter.holder_name ?? "")}</span>
    <RoleTake role="Operator" size="small" label={runFromHere("Operator")} />
  </Banner>
{:else if by}
  <Banner tone="neutral" icon="monitor">
    <span>{runsServiceBar(by)}</span>
    <RoleTake role="Operator" size="small" />
  </Banner>
{/if}
