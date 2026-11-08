<!-- The remote's first screen: the PIN shown on the laptop, and a name for
     this phone or tablet so the laptop can list it (and revoke it) under
     Setup › Devices › Remotes. -->
<script lang="ts">
  import Logo from "../lib/kit/Logo.svelte";
  import { guessName, pinDigits, words } from "../lib/remote.ts";
  import { keepAwake } from "./wake";

  let { revoked = false, onpaired }: { revoked?: boolean; onpaired: () => void } = $props();

  let pin = $state("");
  let name = $state(guessName(navigator.userAgent));
  let busy = $state(false);
  let problem: string | null = $state(null);
  let digits = $derived(pinDigits(pin));

  async function pair(e: SubmitEvent) {
    e.preventDefault();
    if (digits.length !== 6 || busy) return;
    keepAwake();
    busy = true;
    problem = null;
    try {
      const r = await fetch("/remote/pair", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ pin: digits, name }),
      });
      const body = await r.json().catch(() => ({}));
      if (r.ok) {
        onpaired();
        return;
      }
      problem = body.error ?? "The laptop refused the PIN.";
      pin = "";
    } catch {
      problem = "The laptop did not answer. Check this phone or tablet is on the same Wi-Fi as the laptop.";
    } finally {
      busy = false;
    }
  }
</script>

<main class="pair">
  <header>
    <Logo />
    <h1>{words.title}</h1>
  </header>
  {#if revoked}<p class="note" role="status">{words.revoked}</p>{/if}
  <form onsubmit={pair}>
    <p class="lead">{words.pairLead}</p>
    <label class="pin">
      <span class="sr-only">{words.pinLabel}</span>
      <input
        bind:value={pin}
        inputmode="numeric"
        autocomplete="one-time-code"
        enterkeyhint="go"
        maxlength="7"
        placeholder="000 000"
        aria-describedby="pair-problem"
        disabled={busy}
      />
    </label>
    <label class="name">
      <span>{words.nameLabel}</span>
      <input bind:value={name} maxlength="40" autocomplete="off" disabled={busy} />
    </label>
    <button type="submit" disabled={digits.length !== 6 || busy}>{busy ? words.pairing : words.pair}</button>
    <p id="pair-problem" class="problem" role="alert">{problem ?? ""}</p>
  </form>
  <p class="muted scope">{words.pairScope}</p>
</main>

<style>
  .pair {
    max-width: 24rem;
    margin: 0 auto;
    padding: max(1.5rem, env(safe-area-inset-top)) 1rem 2rem;
    display: grid;
    gap: 1.25rem;
  }
  header {
    display: flex;
    align-items: center;
    gap: 0.6rem;
  }
  h1 {
    font-size: 1.15rem;
    font-weight: 600;
  }
  form {
    display: grid;
    gap: 1rem;
  }
  .lead {
    font-size: 1.05rem;
  }
  .pin input {
    width: 100%;
    box-sizing: border-box;
    font-size: 2.4rem;
    letter-spacing: 0.25em;
    text-align: center;
    font-variant-numeric: tabular-nums;
    padding: 0.6rem;
  }
  .name {
    display: grid;
    gap: 0.35rem;
    color: var(--muted);
    font-size: 0.9rem;
  }
  .name input {
    font-size: 1rem;
    padding: 0.7rem 0.75rem;
  }
  button {
    font: inherit;
    font-size: 1.1rem;
    font-weight: 600;
    min-height: 3.5rem;
    border: none;
    border-radius: var(--radius-lg);
    background: var(--gold);
    color: var(--on-gold);
    cursor: pointer;
  }
  button:disabled {
    opacity: 0.6;
    filter: saturate(0);
  }
  .problem {
    min-height: 1.4em;
    color: var(--warn);
  }
  .note {
    border: 1px solid color-mix(in srgb, var(--warn) 30%, transparent);
    border-radius: var(--radius-lg);
    background: var(--warn-soft);
    padding: 0.6rem 0.8rem;
  }
  .scope {
    font-size: 0.85rem;
  }
</style>
