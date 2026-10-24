<!-- The bar that says who is driving whom, across the top of every screen.
     On a driver: "Driving Sanctuary PC" with Stop driving, or a warning
     while the presenter is not connected. On the presenter: who is driving
     it, with Take back control, which always works. -->
<script lang="ts">
  import Banner from "./Banner.svelte";
  import Button from "./Button.svelte";
  import { live, presenterRequest } from "../state.svelte";
  import DriveAction from "./DriveAction.svelte";
  import { driveBarText, drivenBarText, drivenBy, drivingFrom } from "../driving";

  let driving = $derived(drivingFrom(live.team));
  let by = $derived(drivenBy(live.team));
  /** A planning device with a connected presenter, or an ask of its own to show: always one click from driving. */
  let offer = $derived(
    !!live.team && live.team.presenter.role === "Planning" && (live.team.presenter.drive.can_drive || !!live.team.presenter.drive.asking),
  );
</script>

{#if driving}
  <Banner tone={driving.online ? "ready" : "warn"} icon="monitor">
    <span>{driveBarText(driving)}</span>
    <Button size="small" onclick={() => presenterRequest("DriveCancel")}>Stop driving</Button>
  </Banner>
{:else if offer}
  <Banner tone="neutral" icon="monitor">
    <span>{live.team?.presenter.holder_name || "The presenting device"} is presenting.</span>
    <DriveAction size="small" />
  </Banner>
{:else if by}
  <Banner tone="neutral" icon="monitor">
    <span>{drivenBarText(by)}</span>
    <Button size="small" variant="primary" onclick={() => presenterRequest("DriveTakeBack")}>Take back control</Button>
  </Banner>
{/if}
