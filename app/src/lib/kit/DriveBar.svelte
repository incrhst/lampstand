<!-- The bar that says who is driving whom, across the top of every screen.
     On a driver: "Driving Sanctuary PC" with Stop driving, or a warning
     while the presenter is not connected. On the presenter: who is driving
     it, with Take back control, which always works. -->
<script lang="ts">
  import Banner from "./Banner.svelte";
  import Button from "./Button.svelte";
  import { live, presenterRequest } from "../state.svelte";
  import { driveBarText, drivenBarText, drivenBy, drivingFrom } from "../driving";

  let driving = $derived(drivingFrom(live.team));
  let by = $derived(drivenBy(live.team));
</script>

{#if driving}
  <Banner tone={driving.online ? "ready" : "warn"} icon="monitor">
    <span>{driveBarText(driving)}</span>
    <Button size="small" onclick={() => presenterRequest("DriveCancel")}>Stop driving</Button>
  </Banner>
{:else if by}
  <Banner tone="neutral" icon="monitor">
    <span>{drivenBarText(by)}</span>
    <Button size="small" variant="primary" onclick={() => presenterRequest("DriveTakeBack")}>Take back control</Button>
  </Banner>
{/if}
