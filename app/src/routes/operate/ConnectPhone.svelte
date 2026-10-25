<!-- Connect a phone or tablet (T101, decision 0006): a QR code for this
     laptop's remote page, at the address and port the server really has, and
     a fresh PIN in large type with how long it lasts. When a phone pairs the
     dialog names it and closes. "Not loading?" covers the usual causes. -->
<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { onEvent, request, send, type RemotesView } from "../../lib/api";
  import { report } from "../../lib/state.svelte";
  import Button from "../../lib/kit/Button.svelte";
  import Icon from "../../lib/kit/Icon.svelte";

  let { onclose }: { onclose: () => void } = $props();

  let view = $state<RemotesView | null>(null);
  let chosen = $state(0);
  let now = $state(Date.now());
  /** The phone that just paired, by name. */
  let paired = $state<string | null>(null);
  /** Phones paired before the dialog opened, so a new one can be told apart. */
  let known = new Set<string>();

  let pin = $derived(view?.pairing ?? null);
  let address = $derived(view?.addresses[Math.min(chosen, (view?.addresses.length ?? 1) - 1)] ?? null);
  let left = $derived(pin ? Math.max(0, Math.round((pin.expires_ms - now) / 1000)) : 0);

  async function start() {
    try {
      view = await request({ area: "Remote", request: "StartPairing" }, "Remotes");
    } catch (e) {
      report(e);
    }
  }

  function close() {
    if (!paired) send({ area: "Remote", request: "StopPairing" }).catch(report);
    onclose();
  }

  let closing: ReturnType<typeof setTimeout> | undefined;
  const unlisten = onEvent((e) => {
    if (e.type !== "Remotes") return;
    view = e.data;
    const fresh = e.data.sessions.find((s) => !known.has(s.id));
    if (fresh && !paired) {
      paired = fresh.name;
      closing = setTimeout(onclose, 2500);
    }
  });
  const clock = setInterval(() => (now = Date.now()), 1000);

  onMount(async () => {
    try {
      const before = await request({ area: "Remote", request: "Status" }, "Remotes");
      known = new Set(before.sessions.map((s) => s.id));
    } catch (e) {
      report(e);
    }
    await start();
  });
  onDestroy(() => {
    unlisten.then((f) => f());
    clearInterval(clock);
    clearTimeout(closing);
  });

  const spaced = (p: string) => `${p.slice(0, 3)} ${p.slice(3)}`;
  const minutes = (s: number) => `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;

  function onwindowkey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      close();
    }
  }
</script>

<svelte:window onkeydown={onwindowkey} />

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="backdrop" onclick={close}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="connect-title" tabindex="-1" onclick={(e) => e.stopPropagation()}>
    <div class="head">
      <h2 id="connect-title"><Icon name="phone" size={16} />Connect a phone or tablet</h2>
      <Button size="small" variant="ghost" icon="x" label="Close" onclick={close} />
    </div>

    {#if paired}
      <p class="done" role="status"><Icon name="checkCircle" size={18} />{paired} is connected. It can run the slides and the sound.</p>
    {:else if view && !pin}
      <p class="muted">The PIN ran out.</p>
      <div class="actions"><Button variant="primary" onclick={start}>Show a new PIN</Button></div>
    {:else if view && pin}
      {#if address}
        <div class="pair">
          <!-- The core made this SVG from the address; nothing typed is in it. -->
          <div class="qr" aria-hidden="true">{@html address.qr_svg}</div>
          <div class="steps">
            <p>1. On the phone, open the camera and point it at the code, or go to</p>
            <p class="url">{address.url}</p>
            <p>2. Type this PIN:</p>
            <p class="pin" aria-label="PIN {pin.pin.split('').join(' ')}">{spaced(pin.pin)}</p>
            <p class="muted small" role="timer">Works for {minutes(left)}. A new PIN is shown each time.</p>
          </div>
        </div>
        {#if view.addresses.length > 1}
          <div class="others">
            <span class="muted small">This laptop is on more than one network. The phone must be on the same one:</span>
            <div class="chips">
              {#each view.addresses as a, i (a.url)}
                <button type="button" class="chip" aria-pressed={i === chosen} onclick={() => (chosen = i)}>{a.url.replace(/^http:\/\//, "").replace(/\/remote$/, "")}</button>
              {/each}
            </div>
          </div>
        {/if}
      {:else}
        <p class="warn">This laptop is not on a network. Connect it to the Wi-Fi the phones use, then try again.</p>
        <p class="pin small-pin">{spaced(pin.pin)}</p>
      {/if}

      <details>
        <summary>Not loading on the phone?</summary>
        <ul>
          <li><strong>Same network.</strong> The phone must be on the same Wi-Fi as this laptop, not on mobile data or the guest Wi-Fi.</li>
          <li><strong>Guest Wi-Fi.</strong> Many guest networks keep devices apart, so the phone cannot reach the laptop at all.</li>
          <li><strong>Firewall.</strong> If this laptop asked whether to allow Lampstand on the network, allow it (private networks are enough).</li>
          <li><strong>Best:</strong> give the AV team its own Wi-Fi or VLAN for this laptop and its phones, apart from the church's general and guest Wi-Fi.</li>
        </ul>
        <p class="muted small">The page is plain <code>http://</code>, so the phone's own sleep setting may still dim it. Set it to stay on longer if it does.</p>
      </details>
    {:else}
      <p class="muted">Starting…</p>
    {/if}
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: flex;
    justify-content: center;
    align-items: flex-start;
    padding-top: 8vh;
    background: rgb(0 0 0 / 0.5);
  }
  .dialog {
    width: min(40rem, calc(100% - 2rem));
    max-height: 84vh;
    overflow-y: auto;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    box-shadow: 0 1rem 3rem rgb(0 0 0 / 0.5);
    padding: 1rem 1.25rem 1.25rem;
    display: grid;
    gap: 1rem;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  h2 {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 1rem;
    font-weight: 600;
  }
  .pair {
    display: grid;
    grid-template-columns: 13rem 1fr;
    gap: 1.25rem;
    align-items: center;
  }
  .qr {
    background: #fff;
    border-radius: var(--radius);
    padding: 0.4rem;
    line-height: 0;
  }
  .qr :global(svg) {
    width: 100%;
    height: auto;
  }
  .steps {
    display: grid;
    gap: 0.4rem;
  }
  .url {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 0.9rem;
    color: var(--gold);
    word-break: break-all;
  }
  .pin {
    font-size: 3rem;
    font-weight: 700;
    letter-spacing: 0.08em;
    font-variant-numeric: tabular-nums;
    line-height: 1.1;
  }
  .small-pin {
    font-size: 2rem;
  }
  .small {
    font-size: 0.8rem;
  }
  .others {
    display: grid;
    gap: 0.4rem;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
  }
  .chip {
    font: inherit;
    font-size: 0.8rem;
    color: var(--text);
    background: var(--raised);
    border: 1px solid var(--line);
    border-radius: 999px;
    padding: 0.25rem 0.7rem;
    cursor: pointer;
  }
  .chip[aria-pressed="true"] {
    border-color: var(--gold);
    color: var(--gold);
    background: var(--gold-soft);
  }
  details {
    border-top: 1px solid var(--line);
    padding-top: 0.75rem;
    font-size: 0.85rem;
  }
  summary {
    cursor: pointer;
    color: var(--muted);
  }
  details ul {
    margin: 0.6rem 0;
    padding-left: 1.1rem;
    display: grid;
    gap: 0.4rem;
  }
  .done {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    color: var(--ready-text);
    font-size: 1rem;
  }
  .warn {
    color: var(--warn);
  }
  .actions {
    display: flex;
    gap: 0.5rem;
  }
</style>
