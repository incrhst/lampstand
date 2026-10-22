<!-- A section's loop: how long each still slide stays up, and what one pass
     of the section comes to. A video plays through once, so it ignores the
     interval; a video that repeats by itself would hold the loop for good,
     so it is flagged with a way to play it once. -->
<script lang="ts">
  import Button from "../../lib/kit/Button.svelte";
  import { describeLine, formatLength, passLabel, type PassEstimate } from "../../lib/loops";

  let {
    title,
    seconds,
    pass,
    onchange,
    onplayonce,
  }: {
    title: string;
    /** Seconds each still slide stays up; null when the section does not loop. */
    seconds: number | null;
    /** One pass of the section; null until its items have been read. */
    pass: PassEstimate | null;
    onchange: (seconds: number | null) => void;
    onplayonce: (item: string) => void;
  } = $props();

  const choices = [5, 8, 12, 20];
  let custom = $state("");
  $effect(() => {
    custom = seconds && !choices.includes(seconds) ? String(seconds) : "";
  });

  function commitCustom() {
    const n = Math.round(Number(custom));
    if (custom.trim() !== "" && n > 0) onchange(Math.min(n, 3600));
  }
</script>

<div class="panel" role="group" aria-label="Loop {title}">
  <div class="choose">
    <span class="muted">Each still slide stays up for</span>
    <div class="segments">
      <button type="button" class="seg" class:on={seconds === null} aria-pressed={seconds === null} onclick={() => onchange(null)}>Off</button>
      {#each choices as n (n)}
        <button type="button" class="seg" class:on={seconds === n} aria-pressed={seconds === n} onclick={() => onchange(n)}>{n} s</button>
      {/each}
      <input
        class="other"
        type="number"
        min="1"
        max="3600"
        placeholder="Other (s)"
        aria-label="Seconds per slide"
        bind:value={custom}
        onchange={commitCustom}
        onkeydown={(e) => e.key === "Enter" && (e.currentTarget as HTMLInputElement).blur()}
      />
    </div>
  </div>

  {#if seconds === null}
    <p class="muted note">
      Looping is off: this section plays once, in order, as you step through it. Pick an interval to see how long one pass takes.
    </p>
  {:else if pass}
    <div class="total">
      <span class="eyebrow">One pass</span>
      <strong>{passLabel(pass)}</strong>
    </div>
    {#if pass.openEnded.length > 0}
      <p class="muted note">{pass.openEnded.join(", ")} has no fixed length, so this is a minimum.</p>
    {/if}
    {#if pass.lines.length === 0}
      <p class="muted note">Nothing here yet. Add songs, slides or video from the library.</p>
    {:else}
      <ul class="plain lines">
        {#each pass.lines as line (line.id)}
          <li>
            <span class="what">
              <span class="name">{line.title}</span>
              <span class="muted detail">{describeLine(line, seconds)}</span>
            </span>
            <span class="muted time">{line.ms > 0 ? formatLength(line.ms) : "—"}</span>
          </li>
        {/each}
      </ul>
      <p class="back muted">then back to the start</p>
    {/if}
    {#each pass.selfLoops as line (line.id)}
      <div class="notice warn">
        <span>“{line.title}” repeats by itself, so this section would never move on.</span>
        <Button size="small" onclick={() => onplayonce(line.id)}>Play it once instead</Button>
      </div>
    {/each}
    {#if pass.clickSlides > 0}
      <p class="notice">
        <span>
          Animations that wait for a click cannot run in a loop, so those builds come up finished. Animations that start by themselves play first.
        </span>
      </p>
    {/if}
  {/if}
</div>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    padding: 1rem;
    background: var(--panel, var(--raised));
    border: 1px solid var(--gold-line, var(--line-strong));
    border-radius: var(--radius-lg);
    font-size: 0.8125rem;
  }
  .choose {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem 0.75rem;
  }
  .segments {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem;
    align-items: center;
  }
  .seg {
    font: inherit;
    padding: 0.3rem 0.7rem;
    border-radius: var(--radius);
    border: 1px solid var(--line-strong);
    background: var(--raised);
    color: var(--text);
    cursor: pointer;
  }
  .seg.on {
    background: var(--gold);
    border-color: var(--gold);
    color: var(--on-gold, #14161c);
    font-weight: 600;
  }
  .other {
    width: 6.5rem;
    padding: 0.3rem 0.5rem;
    font-size: 0.8125rem;
  }
  .total {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
  }
  .total strong {
    font-size: 1.1rem;
    color: var(--gold);
  }
  .eyebrow {
    font-size: 0.7rem;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--muted);
  }
  .lines {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
  }
  .lines li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    padding: 0.4rem 0.75rem;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--surface, transparent);
  }
  .what {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .detail,
  .back {
    font-size: 0.75rem;
  }
  .note {
    line-height: 1.5;
  }
  .notice {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    padding: 0.6rem 0.75rem;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius);
    line-height: 1.5;
  }
  .notice.warn {
    border-color: var(--gold);
  }
</style>
