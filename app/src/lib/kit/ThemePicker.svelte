<!-- Chooses a theme for a service or a screen. The first choice is "use the
     default", which names the theme that would be used, so the picker says
     what it will look like if nothing is chosen. -->
<script lang="ts">
  import { live } from "../state.svelte";
  import { defaultThemeId } from "../themes";

  let {
    value,
    onchange,
    label,
    inherit = "Default theme",
  }: {
    value: string | null;
    onchange: (id: string | null) => void;
    /** Accessible name. */
    label: string;
    /** What choosing nothing means: "Default theme", "The service's theme". */
    inherit?: string;
  } = $props();

  let fallback = $derived(live.themes.find((t) => t.id === defaultThemeId(live.settings, live.themes)));
  // A theme that was deleted shows as the default, which is what the core uses.
  let current = $derived(value && live.themes.some((t) => t.id === value) ? value : "");
</script>

<select
  aria-label={label}
  value={current}
  onchange={(e) => onchange(e.currentTarget.value || null)}
  title="Themes are made in Setup › Themes"
>
  <option value="">{inherit}{inherit === "Default theme" && fallback ? ` (${fallback.name})` : ""}</option>
  {#each live.themes as t (t.id)}
    <option value={t.id}>{t.name}</option>
  {/each}
</select>
