<!-- Who has a role, for its row in Setup › Devices › Who runs what: the
     holder's name with its role bar, and, when this device could not check
     the role's newest record (#184), that it can't, and what to do. -->
<script lang="ts">
  import type { RoleName, TeamStatus } from "../api";
  import RoleBar from "./RoleBar.svelte";
  import { UNCHECKED_HELP, heldRoles, holderLine, roleOf, rolesHeldBy, uncheckedLine, uncheckedRoles } from "../roles";

  let { team, role }: { team: TeamStatus; role: RoleName } = $props();
  let r = $derived(roleOf(team, role));
  let held = $derived(!r ? [] : r.holds ? heldRoles(team) : r.holder ? rolesHeldBy(team, r.holder) : []);
</script>

{#if r}
  <span class="holder">
    {#if r.holds || r.holder}<RoleBar {held} unchecked={uncheckedRoles(team)} />{/if}
    <span class="muted">{holderLine(r)}</span>
  </span>
  {#if r.unverified}
    <span class="unchecked" role="note">
      <strong>{uncheckedLine([role])}.</strong>
      {UNCHECKED_HELP}
    </span>
  {/if}
{/if}

<style>
  .holder {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
  }
  .muted {
    color: var(--muted);
  }
  .unchecked {
    display: block;
    font-size: 0.8rem;
    color: var(--warn);
  }
</style>
