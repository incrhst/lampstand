<!-- Bare devices screen (T17): pair this device with another and see which
     paired devices are connected. The presenting machine shows a code; the
     service device types it. Teams and roles (T18) replace pairing. -->
<script lang="ts">
  import { onDestroy } from "svelte";
  import { onEvent, request, type TeamRequest, type TeamStatus } from "../../lib/api";
  import { report } from "../../lib/state.svelte";
  import Button from "../../lib/kit/Button.svelte";
  import Panel from "../../lib/kit/Panel.svelte";

  let status: TeamStatus | null = $state(null);
  let code = $state("");
  let address = $state("");
  let busy = $state(false);
  let now = $state(Date.now());

  async function team(req: TeamRequest) {
    try {
      status = await request({ area: "Team", request: req }, "Team");
      return true;
    } catch (e) {
      report(e);
      return false;
    }
  }

  async function join() {
    busy = true;
    const ok = await team({ Join: { invite: code, address: address.trim() || null } });
    busy = false;
    if (ok) {
      code = "";
      address = "";
    }
  }

  async function connect() {
    busy = true;
    if (await team({ Connect: { address: address.trim() } })) address = "";
    busy = false;
  }

  const refresh = () => team("Status");
  const unlisten = onEvent((e) => {
    if (e.type === "SyncStatus") refresh();
  });
  const clock = setInterval(() => {
    now = Date.now();
    if (status?.pairing && status.pairing.expires_ms <= now) refresh();
  }, 1000);
  onDestroy(() => {
    unlisten.then((f) => f());
    clearInterval(clock);
  });
  refresh();

  function spaced(c: string) {
    return `${c.slice(0, 3)} ${c.slice(3)}`;
  }
  function minutes(ms: number) {
    const s = Math.max(0, Math.round((ms - now) / 1000));
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
  }
</script>

<div class="grid">
  <Panel title="This device">
    {#if status}
      <p><strong>{status.name}</strong> <small class="muted">key {status.fingerprint}</small></p>
      {#if status.addresses.length}
        <p class="muted">Address: {status.addresses.join(", ")}</p>
      {/if}
      {#if status.pairing}
        <p class="muted">On the other device, open Devices and type:</p>
        <p class="code" aria-label="Pairing code">{spaced(status.pairing.code)}</p>
        <p class="muted">Valid for {minutes(status.pairing.expires_ms)}.</p>
        <div class="actions"><Button onclick={() => team("StopPairing")}>Cancel</Button></div>
      {:else}
        <p class="muted">To let a planner's laptop or the office PC send changes here, show a pairing code.</p>
        <div class="actions"><Button variant="primary" onclick={() => team("StartPairing")}>Pair a device</Button></div>
      {/if}
    {/if}
  </Panel>

  <Panel title="Join another device">
    <form
      onsubmit={(e) => {
        e.preventDefault();
        join();
      }}
    >
      <label>
        Pairing code
        <input bind:value={code} inputmode="numeric" autocomplete="off" placeholder="123 456" />
      </label>
      <label>
        Address <small class="muted">(only if it is not found automatically)</small>
        <input bind:value={address} autocomplete="off" placeholder="192.168.1.20:7340" />
      </label>
      <span class="row">
        <Button variant="primary" disabled={busy || code.replace(/\D/g, "").length !== 6} onclick={join}>Pair</Button>
        <Button disabled={busy || !address.trim()} onclick={connect} title="Reconnect to a paired device by address">
          Connect
        </Button>
      </span>
    </form>
    {#if status?.nearby.length}
      <h3>Nearby</h3>
      <ul class="list">
        {#each status.nearby as n (n.device)}
          <li>
            <span>
              {n.name} <small class="muted">{n.address} · key {n.fingerprint}</small>
            </span>
            {#if n.pairing}<small class="badge">showing a code</small>{/if}
          </li>
        {/each}
      </ul>
    {/if}
  </Panel>

  <Panel title="Paired devices">
    {#snippet actions()}
      <Button onclick={refresh}>Refresh</Button>
    {/snippet}
    <ul class="list">
      {#each status?.paired ?? [] as p (p.device)}
        <li>
          <span>
            <span class="dot" class:on={p.connected} aria-hidden="true"></span>
            <strong>{p.name}</strong>
            <small class="muted">{p.connected ? "connected" : "not connected"}{p.address ? ` · ${p.address}` : ""} · key {p.fingerprint}</small>
          </span>
          <Button onclick={() => team({ Unpair: { device: p.device } })}>Unpair</Button>
        </li>
      {:else}
        <li class="muted">No paired devices yet.</li>
      {/each}
    </ul>
  </Panel>
</div>

<style>
  .actions {
    margin-top: 1rem;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(18rem, 1fr));
    gap: 1rem;
    align-items: start;
  }
  form {
    display: grid;
    gap: 0.6rem;
  }
  label {
    display: grid;
    gap: 0.2rem;
  }
  input {
    font: inherit;
    padding: 0.4rem 0.6rem;
    border-radius: 6px;
    border: 1px solid var(--line);
    background: var(--raised);
    color: var(--text);
  }
  .row {
    display: flex;
    gap: 0.6rem;
  }
  .code {
    font-size: 2.4rem;
    letter-spacing: 0.15em;
    font-variant-numeric: tabular-nums;
    margin: 0.2rem 0;
    color: var(--accent);
  }
  h3 {
    font-size: 0.85rem;
    color: var(--muted);
    margin: 1rem 0 0.4rem;
  }
  .badge {
    color: var(--accent);
  }
  .dot {
    display: inline-block;
    width: 0.6rem;
    height: 0.6rem;
    border-radius: 50%;
    background: var(--muted);
    margin-right: 0.3rem;
  }
  .dot.on {
    background: var(--live);
  }
</style>
