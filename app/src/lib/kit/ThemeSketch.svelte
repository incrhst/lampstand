<!-- A theme drawn in the page, for when the real picture from the core is not
     available (no GPU yet, or still loading): the background colour, the
     font, size, colour and alignment, at the screen's shape. It is an
     approximation; the picture from the core is the truth. -->
<script lang="ts">
  import type { Theme } from "../api";
  import { backgroundKind, toHex } from "../themes";

  let {
    theme,
    lines,
    footer = "",
    width = 1920,
    height = 1080,
  }: { theme: Theme; lines: readonly string[]; footer?: string; width?: number; height?: number } = $props();

  let bg = $derived(theme.background);
  let color = $derived(typeof bg === "object" && "Color" in bg ? toHex(bg.Color) : "#000");
  let area = $derived(theme.text_area);
  let t = $derived(theme.text);
  const justifyBy = { Top: "flex-start", Middle: "center", Bottom: "flex-end" } as const;
  let justify = $derived(justifyBy[t.valign]);
  // Sizes are pixels on a 1080-pixel-tall screen; the sketch is `cqh` of its own height.
  let unit = $derived(100 / 1080);
</script>

<div class="sketch" style="aspect-ratio:{width}/{height};background:{color}" role="img" aria-label="Sketch of the theme">
  {#if backgroundKind(bg) === "Image" || backgroundKind(bg) === "Video"}
    <span class="media">{backgroundKind(bg) === "Image" ? "Picture" : "Video"} background</span>
  {/if}
  <div
    class="text"
    style="left:{area.x * 100}%;top:{area.y * 100}%;width:{area.width * 100}%;height:{area.height * 100}%;
      justify-content:{justify};text-align:{t.align === 'Center' ? 'center' : t.align.toLowerCase()};
      font-family:'{t.font_family}',sans-serif;font-size:{t.size * unit}cqh;font-weight:{t.weight};
      font-style:{t.italic ? 'italic' : 'normal'};color:{toHex(t.color)};line-height:{t.line_spacing}"
  >
    {#each lines.slice(0, theme.max_lines) as line}<span>{line}</span>{/each}
    {#if footer && theme.show_copyright}<small style="font-size:{t.size * 0.4 * unit}cqh">{footer}</small>{/if}
  </div>
</div>

<style>
  .sketch {
    position: relative;
    width: 100%;
    overflow: hidden;
    container-type: size;
    background: #000;
  }
  .text {
    position: absolute;
    display: flex;
    flex-direction: column;
    padding: 1%;
    box-sizing: border-box;
  }
  small {
    margin-top: 0.4em;
    opacity: 0.8;
  }
  .media {
    position: absolute;
    inset: auto 0.5rem 0.4rem auto;
    font-size: 0.7rem;
    color: #fff;
    opacity: 0.6;
  }
</style>
