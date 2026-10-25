<!-- Chooses what a screen shows when nothing is on air: the default (your
     logo and name, else the Lampstand lamp), the theme's plain background, a
     picture, a looping video (silent) or one slide of a deck.
     Used for every screen at once, and for one screen, which can also say
     "same as the other screens". Black still covers it; Idle puts it on air
     mid-service. -->
<script lang="ts">
  import { open as pickFiles } from "@tauri-apps/plugin-dialog";
  import { request, type Deck, type IdleScreen } from "../api";
  import { describeIdle, idleKind, stepSlide, type IdleKind } from "../idle";
  import { loadIdlePicture } from "../idleThumb";
  import { imageExtensions, onePath, videoExtensions } from "../media";
  import { live, report } from "../state.svelte";
  import Button from "./Button.svelte";
  import Segmented from "./Segmented.svelte";

  let {
    value,
    onchange,
    label,
    allowInherit = false,
  }: {
    value: IdleScreen | null;
    onchange: (idle: IdleScreen | null) => void;
    label: string;
    /** A screen can use the setting for all screens. */
    allowInherit?: boolean;
  } = $props();

  let decks: Deck[] = $state([]);
  let picture: string | null = $state(null);
  let slides = $state(0);
  let busy = $state(false);

  let kind = $derived(idleKind(value));
  const options = $derived([
    ...(allowInherit ? [{ id: "Inherit", label: "Same as other screens" }] : []),
    { id: "Default", label: "Default" },
    { id: "Blank", label: "Plain" },
    { id: "Image", label: "Picture" },
    { id: "Video", label: "Video" },
    { id: "DeckSlide", label: "Slide" },
  ] as { id: IdleKind; label: string }[]);

  request({ area: "Library", request: "ListDecks" }, "Decks")
    .then((all) => (decks = all))
    .catch(report);

  // A small picture of what is chosen.
  $effect(() => {
    const v = value;
    picture = null;
    let current = true;
    loadIdlePicture(v, 480, live.settings?.brand ?? null, live.settings?.default_theme ?? null).then((r) => {
      if (!current) return;
      picture = r?.picture ?? null;
      if (r?.slides !== undefined) slides = r.slides;
    });
    return () => {
      current = false;
    };
  });

  async function choose(next: IdleKind) {
    if (next === "Inherit") return onchange(null);
    if (next === "Blank" || next === "Default") return onchange(next);
    if (next === "DeckSlide") {
      const deck = decks[0];
      if (!deck) {
        live.status = "Add a PowerPoint deck in Plan first, then choose one of its slides here.";
        return;
      }
      return onchange({ DeckSlide: { deck: deck.id, slide: 0 } });
    }
    await pick(next);
  }

  /** Copies a picture or video into the library and uses it. */
  async function pick(as: "Image" | "Video") {
    try {
      const video = as === "Video";
      const path = onePath(
        await pickFiles({
          title: video ? "Choose a video to loop" : "Choose a picture",
          filters: [{ name: video ? "Video" : "Pictures", extensions: video ? videoExtensions : imageExtensions }],
        }),
      );
      if (!path) return;
      busy = true;
      live.status = "Copying into the library…";
      const file = await request({ area: "Library", request: { AddMediaFile: { path } } }, "Media");
      onchange(video ? { Video: { blob: file.blob, name: file.name } } : { Image: { blob: file.blob, name: file.name } });
      live.status = `Added ${file.name}`;
    } catch (e) {
      report(e);
    } finally {
      busy = false;
    }
  }

  function setDeck(deck: string) {
    onchange({ DeckSlide: { deck, slide: 0 } });
  }
  function step(by: number) {
    if (value && typeof value === "object" && "DeckSlide" in value) {
      onchange({ DeckSlide: { deck: value.DeckSlide.deck, slide: stepSlide(value.DeckSlide.slide, by, slides) } });
    }
  }
</script>

<div class="idle">
  <Segmented {label} {options} value={kind} onchange={choose} />
  <div class="detail">
    {#if picture}
      <img class="picture" src={picture} alt="" />
    {/if}
    <div class="text">
      <p class="muted small">{describeIdle(value, decks, undefined, live.settings?.brand ?? null)}</p>
      {#if value && typeof value === "object" && "Image" in value}
        <Button size="small" onclick={() => pick("Image")} disabled={busy}>Choose another picture…</Button>
      {:else if value && typeof value === "object" && "Video" in value}
        <Button size="small" onclick={() => pick("Video")} disabled={busy}>Choose another video…</Button>
        <p class="faint small">Plays on a loop with the sound off.</p>
      {:else if value && typeof value === "object" && "DeckSlide" in value}
        {@const slide = value.DeckSlide}
        <select aria-label="Deck" value={slide.deck} onchange={(e) => setDeck(e.currentTarget.value)}>
          {#each decks as d (d.id)}<option value={d.id}>{d.title}</option>{/each}
        </select>
        <div class="stepper">
          <Button size="small" icon="prev" label="Previous slide" onclick={() => step(-1)} disabled={slide.slide === 0} />
          <span class="num">Slide {slide.slide + 1}{slides ? ` of ${slides}` : ""}</span>
          <Button size="small" icon="next" label="Next slide" onclick={() => step(1)} disabled={slides > 0 && slide.slide >= slides - 1} />
        </div>
      {/if}
    </div>
  </div>
</div>

<style>
  .idle {
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }
  .detail {
    display: flex;
    gap: 0.9rem;
    align-items: flex-start;
  }
  .picture {
    width: 9.5rem;
    aspect-ratio: 16 / 9;
    object-fit: cover;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: #000;
  }
  .text {
    display: flex;
    flex-direction: column;
    gap: 0.45rem;
    align-items: flex-start;
    font-size: 0.85rem;
  }
  .small {
    font-size: 0.8rem;
  }
  .stepper {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
</style>
