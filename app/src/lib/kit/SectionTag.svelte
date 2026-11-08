<!-- A slide's label, coloured when it names a song section (verse, chorus,
     pre-chorus, bridge), so the parts of a song stand out on the wall. -->
<script lang="ts">
  import { sectionTone } from "../sections";

  let { label }: { label: string } = $props();
  let tone = $derived(sectionTone(label));
</script>

{#if tone}
  <span class="tag {tone}">{label}</span>
{:else}
  <span>{label}</span>
{/if}

<style>
  .tag {
    display: inline-block;
    padding: 0.05rem 0.4rem;
    border-radius: 4px;
    font-weight: 600;
    color: var(--tone);
    background: color-mix(in srgb, var(--tone) 16%, transparent);
  }
  .verse {
    --tone: var(--section-verse);
  }
  .chorus {
    --tone: var(--section-chorus);
  }
  .prechorus {
    --tone: var(--section-prechorus);
  }
  .bridge {
    --tone: var(--section-bridge);
  }
</style>
