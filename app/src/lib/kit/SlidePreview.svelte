<!-- A slide drawn in the operator view: 16:9, black, text centred inside
     the output's safe area, with an optional dashed guide for it. The
     real output is rendered by the core; this is the operator's text
     picture until thumbnails arrive (T47). -->
<script lang="ts">
  import type { SafeArea } from "../api";

  let {
    lines = [],
    overlay = "None",
    safeArea,
    showGuide = false,
    size = "normal",
    empty = "",
  }: {
    lines?: readonly string[];
    overlay?: "None" | "Black" | "Logo";
    /** Margins as fractions (0.05 = 5%). */
    safeArea?: SafeArea | null;
    showGuide?: boolean;
    size?: "large" | "normal" | "small";
    /** Shown when there are no lines. */
    empty?: string;
  } = $props();

  let box = $derived.by(() => {
    const s = safeArea ?? { top: 0, right: 0, bottom: 0, left: 0 };
    return `top:${s.top * 100}%;right:${s.right * 100}%;bottom:${s.bottom * 100}%;left:${s.left * 100}%`;
  });
</script>

<div class="screen {size}">
  {#if overlay === "Logo"}
    <!-- The ring the compositor draws until a church logo can be set. -->
    <div class="cover"><span class="logo" aria-label="Logo"></span></div>
  {:else if overlay === "None"}
    <div class="text" style={box}>
      {#each lines as line}<p>{line}</p>{:else}<span class="empty">{empty}</span>{/each}
    </div>
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
  .guide {
    position: absolute;
    border: 1px dashed var(--gold);
    pointer-events: none;
  }
</style>
