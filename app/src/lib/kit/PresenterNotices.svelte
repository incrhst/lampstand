<!-- Messages about who runs what that need an answer or should not be
     missed, shown across the top of every screen. The role prompts (a
     device asking this one to hand a role over, this device's own ask, a
     role lost while away) are in RolePrompts; this adds the notice that the
     screens' device took the service back, the note that this device
     now runs the screens, and the warning ahead of a sound that will not
     play because nobody runs the sound (#182). -->
<script lang="ts">
  import Banner from "./Banner.svelte";
  import Button from "./Button.svelte";
  import { live, presenterRequest } from "../state.svelte";
  import { RUNS_SCREENS_NOW, operatorTakenBack, operatorTakenBackNotice } from "../roles";
  import { nobodyWarning, soundQueue } from "../audio";
  import { remoteSounds } from "../remote";
  import DriveBar from "./DriveBar.svelte";
  import RolePrompts from "./RolePrompts.svelte";

  let p = $derived(live.team?.presenter ?? null);
  let allowed = $state(false);
  /** "Nobody runs the sound, so Offering music won't play.", for the next sound the service reaches. */
  let nobody = $derived(
    nobodyWarning(
      live.sound,
      soundQueue(remoteSounds(live.loaded), live.loaded?.items.map((i) => i.id) ?? [], live.live?.item ?? null),
    ),
  );
  let wasAsking = false;
  // An ask that ends with this device running the screens: say so once.
  $effect(() => {
    const asking = !!p?.asking && !p.asking.declined;
    if (wasAsking && !asking && p?.role === "Presenting") {
      allowed = true;
      setTimeout(() => (allowed = false), 8000);
    }
    wasAsking = asking;
  });
</script>

<DriveBar />

<RolePrompts />

{#if nobody}
  <div class="notices">
    <Banner tone="warn" icon="sound">{nobody}</Banner>
  </div>
{/if}

{#if p && ((p.drive.taken_back_by && operatorTakenBack(live.team)) || allowed)}
  <div class="notices">
    {#if p.drive.taken_back_by && operatorTakenBack(live.team)}
      <Banner tone="warn">
        <span>{operatorTakenBackNotice(p.drive.taken_back_by)}</span>
        <Button size="small" variant="ghost" onclick={() => presenterRequest("DriveDismiss")}>OK</Button>
      </Banner>
    {/if}
    {#if allowed}
      <Banner tone="ready">{RUNS_SCREENS_NOW}</Banner>
    {/if}
  </div>
{/if}

<style>
  .notices {
    display: grid;
    gap: 0.4rem;
    padding: 0.5rem 1.25rem 0;
  }
</style>
