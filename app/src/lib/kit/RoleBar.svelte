<!-- A thin bar in the role colours: one equal segment for each role a
     device holds, always Projection, Operator, Stream, Audio. A role whose
     record this device could not check (#184) is hatched. A device that runs
     nothing shows an empty bar, so the row does not jump. The words are the
     roster's (lib/roles.ts). -->
<script lang="ts">
  import type { RoleName } from "../api";
  import { roleBar } from "../roles";

  let { held, unchecked = [] }: { held: RoleName[]; unchecked?: RoleName[] } = $props();
  let bar = $derived(roleBar(held, unchecked));
</script>

<span class="bar" class:empty={bar.segments.length === 0} role="img" aria-label={bar.label} title={bar.label}>
  {#each bar.segments as s (s.role)}
    <span class="segment {s.role.toLowerCase()}" class:unchecked={s.unchecked} data-role={s.role}></span>
  {/each}
</span>

<style>
  .bar {
    display: inline-flex;
    flex-shrink: 0;
    width: 2rem;
    height: 0.3rem;
    gap: 1px;
    border-radius: 999px;
    overflow: hidden;
    vertical-align: middle;
  }
  .empty {
    background: var(--line-strong);
  }
  .segment {
    flex: 1;
  }
  .projection {
    background: var(--role-projection);
  }
  .operator {
    background: var(--role-operator);
  }
  .stream {
    background: var(--role-stream);
  }
  .audio {
    background: var(--role-audio);
  }
  .unchecked {
    background-image: repeating-linear-gradient(135deg, transparent 0 2px, var(--canvas) 2px 4px);
  }
</style>
