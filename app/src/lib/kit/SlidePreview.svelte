<!-- A slide drawn in the operator view: 16:9, black, text centred inside
     the output's safe area, with an optional dashed guide for it. Deck
     slides show the core's rendered picture (`image`); text slides are the
     operator's text picture until thumbnails for them arrive (T47). -->
<script lang="ts">
  import type { LogoBug, SafeArea } from "../api";

  let {
    lines = [],
    footer = null,
    overlay = "None",
    safeArea,
    showGuide = false,
    size = "normal",
    empty = "",
    image = null,
    idle = null,
    brand = null,
  }: {
    lines?: readonly string[];
    /** The smaller line at the foot: attribution, copyright, a verse reference. */
    footer?: string | null;
    overlay?: "None" | "Black" | "Logo";
    /** Margins as fractions (0.05 = 5%). */
    safeArea?: SafeArea | null;
    showGuide?: boolean;
    size?: "large" | "normal" | "small";
    /** Shown when there are no lines. */
    empty?: string;
    /** A rendered picture of the slide (a `data:` URL), drawn instead of lines. */
    image?: string | null;
    /** A picture of the screen's idle screen, shown when there is nothing to show. */
    idle?: string | null;
    /** The church's logo screen, for the Logo overlay. */
    brand?: { logo: string | null; background: string; accent: string; bug?: LogoBug } | null;
  } = $props();

  // The logo bug, drawn where the screens draw it: in the safe area's lower
  // corner, over the slide, hidden under Black and the Logo screen.
  const bugHeight = { Small: 0.07, Medium: 0.1, Large: 0.14 } as const;
  let bug = $derived(brand?.logo && brand.bug?.enabled && overlay === "None" ? { src: brand.logo, ...brand.bug } : null);
  let bugBox = $derived.by(() => {
    if (!bug) return "";
    const s = safeArea ?? { top: 0, right: 0, bottom: 0, left: 0 };
    const edge = bug.corner === "LowerLeft" ? `left:${s.left * 100}%` : `right:${s.right * 100}%`;
    // 16:9, so 2% of the height is 1.125% of the width.
    return `${edge};bottom:${s.bottom * 100}%;margin:1.125cqw;height:${bugHeight[bug.size] * 56.25}cqw;max-width:25%;opacity:${Math.max(10, bug.opacity) / 100}`;
  });

  let box = $derived.by(() => {
    const s = safeArea ?? { top: 0, right: 0, bottom: 0, left: 0 };
    return `top:${s.top * 100}%;right:${s.right * 100}%;bottom:${s.bottom * 100}%;left:${s.left * 100}%`;
  });
</script>

<div class="screen {size}">
  {#if overlay === "Logo"}
    <!-- The church's logo on its colour; a ring until it has a logo, as the screens show. -->
    <div class="cover" style={brand ? `background:${brand.background}` : ""}>
      {#if brand?.logo}
        <img class="mark" src={brand.logo} alt="Logo" />
      {:else}
        <span class="logo" aria-label="Logo" style={brand ? `border-color:${brand.accent}` : ""}></span>
      {/if}
    </div>
  {:else if overlay === "None" && idle && lines.length === 0 && !image}
    <img class="picture idle" src={idle} alt="Idle screen" />
    <span class="idle-tag">Idle screen</span>
  {:else if overlay === "None" && image}
    <img class="picture" src={image} alt="" />
  {:else if overlay === "None"}
    <div class="text" style={box}>
      {#each lines as line}<p>{line}</p>{:else}<span class="empty">{empty}</span>{/each}
      {#if footer && lines.length > 0}<p class="footer">{footer}</p>{/if}
    </div>
  {/if}
  {#if bug}
    <img class="bug {bug.corner === 'LowerLeft' ? 'left' : 'right'}" src={bug.src} alt="" style={bugBox} />
  {/if}
  {#if showGuide}<div class="guide" style={box} aria-hidden="true"></div>{/if}
</div>

<style>
  .screen {
    position: relative;
    aspect-ratio: 16 / 9;
    width: 100%;
    background: #000;
    overflow: hidden;
    container-type: inline-size;
    user-select: none;
  }
  .text {
    position: absolute;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    text-align: center;
    padding: 2%;
    color: #fff;
    font-weight: 600;
    line-height: 1.3;
    text-shadow: 0 1px 3px rgba(0, 0, 0, 0.8);
  }
  .large .text {
    font-size: max(0.9rem, 3.4cqw);
  }
  .normal .text {
    font-size: max(0.6rem, 3.8cqw);
  }
  .small .text {
    font-size: max(0.45rem, 4.2cqw);
    font-weight: 500;
  }
  p {
    margin: 0;
  }
  /* Where the screen draws it: small, at the foot of the safe area. */
  .footer {
    position: absolute;
    right: 2%;
    bottom: 3%;
    left: 2%;
    font-size: 0.55em;
    font-weight: 400;
    opacity: 0.8;
  }
  .empty {
    color: var(--faint);
    font-weight: 400;
    font-size: 0.8rem;
    text-shadow: none;
  }
  .cover {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .logo {
    width: 16%;
    aspect-ratio: 1;
    border-radius: 50%;
    border: max(2px, 1cqw) solid #fff;
  }
  .mark {
    max-width: 60%;
    max-height: 60%;
    object-fit: contain;
  }
  .picture.idle {
    object-fit: cover;
  }
  .idle-tag {
    position: absolute;
    left: 0.4rem;
    bottom: 0.4rem;
    padding: 0.1rem 0.4rem;
    border-radius: 3px;
    background: rgb(0 0 0 / 0.6);
    color: #fff;
    font-size: 0.7rem;
  }
  .picture {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: contain;
  }
  .bug {
    position: absolute;
    width: auto;
    object-fit: contain;
    pointer-events: none;
  }
  .bug.left {
    object-position: left bottom;
  }
  .bug.right {
    object-position: right bottom;
  }
  .guide {
    position: absolute;
    border: 1px dashed var(--gold);
    pointer-events: none;
  }
</style>
