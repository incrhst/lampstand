<!-- Edits a text slide item inline: a title for the order list, the body,
     and an optional attribution line for a quote. A blank line, or a line
     of three hyphens, starts a new slide. Saves when a field loses focus. -->
<script lang="ts">
  import { splitText } from "../../lib/textSlides";

  let {
    title,
    body,
    attribution,
    label,
    onsave,
  }: {
    title: string;
    body: string;
    attribution: string | null;
    /** Names the item, for the fields' accessible labels. */
    label: string;
    onsave: (title: string, body: string, attribution: string | null) => void;
  } = $props();

  let titleText = $state("");
  let bodyText = $state("");
  let attributionText = $state("");
  $effect(() => {
    titleText = title;
    bodyText = body;
    attributionText = attribution ?? "";
  });

  let count = $derived(splitText(bodyText).length);

  function commit() {
    const a = attributionText.trim() || null;
    if (titleText !== title || bodyText !== body || a !== (attribution ?? null)) onsave(titleText, bodyText, a);
  }
</script>

<div class="text-editor">
  <label class="field">
    <span class="eyebrow">Title <span class="muted">(shown in the order, not on screen)</span></span>
    <input type="text" aria-label="Title of {label}" bind:value={titleText} onchange={commit} />
  </label>
  <label class="field">
    <span class="eyebrow">
      Text <span class="muted">· {count === 1 ? "1 slide" : `${count} slides`}. A blank line starts a new slide.</span>
    </span>
    <textarea rows="6" aria-label="Text of {label}" bind:value={bodyText} onchange={commit}></textarea>
  </label>
  <label class="field">
    <span class="eyebrow">Attribution <span class="muted">(optional, for a quote)</span></span>
    <input type="text" aria-label="Attribution of {label}" placeholder="C. S. Lewis" bind:value={attributionText} onchange={commit} />
  </label>
</div>

<style>
  .text-editor {
    display: grid;
    gap: var(--space-2, 8px);
  }
  .field {
    display: grid;
    gap: 4px;
  }
  textarea {
    font: inherit;
    resize: vertical;
    min-height: 6em;
  }
</style>
