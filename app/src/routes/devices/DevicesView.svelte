<!-- Devices screen (T17): pair this device with another, see which paired
     devices are connected and when changes last moved, who runs the screens,
     the service, the stream and the sound (the roster, with a Take button for
     each and the takeover time), and what each device runs. One device shows
     a code; the other types it. Teams (T18) replace pairing. -->
<script lang="ts">
  import { onDestroy } from "svelte";
  import { onEvent, request, type RemotesView, type TeamRequest, type TeamStatus } from "../../lib/api";
  import { report } from "../../lib/state.svelte";
  import Button from "../../lib/kit/Button.svelte";
  import Panel from "../../lib/kit/Panel.svelte";
  import RoleTake from "../../lib/kit/RoleTake.svelte";
  import RoleBar from "../../lib/kit/RoleBar.svelte";
  import RoleHolder from "../../lib/kit/RoleHolder.svelte";
  import ConnectRemote from "../operate/ConnectRemote.svelte";
  import {
    ALWAYS_ALLOWED,
    PANEL_TITLE,
    ROLES,
    TAKEOVER_HELP,
    TAKEOVER_LABEL,
    deviceRunsLine,
    heldRoles,
    ownRunsLine,
    roleOf,
    rolesHeldBy,
    uncheckedRoles,
    runs,
    stopAlways,
    takeoverOption,
  } from "../../lib/roles";
  import { TAKEOVER_CHOICES } from "../../lib/presenting";
  import { ago, syncPill } from "../../lib/sync";

  let status = $state<TeamStatus | null>(null);
  let code = $state("");
  let address = $state("");
  let busy = $state(false);
  /** The address field, for a network that hides computers from each other. */
  let byAddress = $state(false);
  const onMac = typeof navigator !== "undefined" && /mac/i.test(navigator.platform);
  /** Computers nearby showing a code, the ones the code is typed for. */
  let ready = $derived(status?.nearby.filter((n) => n.pairing) ?? []);
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
  let presenter = $derived.by(() => status?.presenter ?? null);
  /** The saved time, plus the current one if it is not among the choices. */
  let takeoverChoices = $derived(
    [...new Set<number>([...TAKEOVER_CHOICES, presenter?.takeover_secs ?? 120])].sort((a, b) => a - b),
  );
  // Phones and tablets paired as Lampstand Remote (T99, T101).
  let remotes: RemotesView | null = $state(null);
  let connecting = $state(false);
  async function remote(req: "Status" | { Revoke: { id: string } }) {
    try {
      remotes = await request({ area: "Remote", request: req }, "Remotes");
    } catch (e) {
      report(e);
    }
  }
  const unlisten = onEvent((e) => {
    if (e.type === "SyncStatus") refresh();
    if (e.type === "Remotes") remotes = e.data;
  });
  remote("Status");
  const clock = setInterval(() => {
    now = Date.now();
    if (status?.pairing && status.pairing.expires_ms <= now) refresh();
  }, 1000);
  onDestroy(() => {
    unlisten.then((f) => f());
    clearInterval(clock);
  });
  refresh();

  const pillOf = (s: TeamStatus | null) => syncPill(s?.sync ?? null);

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
      {#if status.paired.length > 0}
        <p class="muted"><RoleBar held={heldRoles(status)} unchecked={uncheckedRoles(status)} /> {ownRunsLine(status)}</p>
      {/if}
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
      {#if ready.length === 1}
        <p class="ready">Ready to pair with <strong>{ready[0].name}</strong>. Type the code it shows.</p>
      {:else if ready.length > 1}
        <p class="ready">{ready.length} computers nearby are showing a code. Type the one you want to pair with.</p>
      {:else}
        <p class="muted">On the other computer, open Setup › Devices and choose Pair a device. Then type the code it shows here.</p>
      {/if}
      <label>
        Pairing code
        <input bind:value={code} inputmode="numeric" autocomplete="off" placeholder="123 456" />
      </label>
      {#if byAddress}
        <label>
          Its address <small class="muted">(shown on that computer under This device)</small>
          <input bind:value={address} autocomplete="off" placeholder="192.168.1.20:7340" />
        </label>
      {/if}
      <span class="row">
        <Button variant="primary" disabled={busy || code.replace(/\D/g, "").length !== 6} onclick={join}>Pair</Button>
        {#if byAddress}
          <Button disabled={busy || !address.trim()} onclick={connect} title="Reconnect to a paired device by address">
            Connect
          </Button>
        {:else}
          <button type="button" class="link" onclick={() => (byAddress = true)}>Can't find it?</button>
        {/if}
      </span>
    </form>
    {#if byAddress || (status && status.nearby.length === 0)}
      <!-- Pairing never needs an address when the computers can see each
           other; when they can't, say why before asking for one. -->
      <p class="muted small">
        Computers find each other on the same network. If the other one isn't found, check that both are on the
        same Wi-Fi or network, not a guest Wi-Fi that keeps devices apart.{#if onMac}{" "}On a Mac, Lampstand needs <strong>Local Network</strong> access: System Settings › Privacy &amp; Security ›
          Local Network.{/if}
      </p>
    {/if}
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

  {#if status && status.paired.length > 0}
    <Panel title={PANEL_TITLE}>
      <ul class="list roles">
        {#each ROLES as name (name)}
          {@const r = roleOf(status, name)}
          {#if r}
            <li>
              <span>
                <strong>{runs(name)}</strong>
                <RoleHolder team={status} role={name} />
                {#each r.always as d (d.device)}
                  <small class="muted">{ALWAYS_ALLOWED}: {d.name}</small>
                  <Button size="small" variant="ghost" onclick={() => team({ RoleAlways: { role: name, device: d.device, always: false } })}>{stopAlways(d.name)}</Button>
                {/each}
              </span>
              <RoleTake role={name} size="small" />
            </li>
          {/if}
        {/each}
      </ul>
      {#if presenter}
        <label class="takeover">
          {TAKEOVER_LABEL}
          <select
            value={presenter.takeover_secs}
            onchange={(e) => team({ SetTakeover: { seconds: Number(e.currentTarget.value) } })}
          >
            {#each takeoverChoices as s}
              <option value={s}>{takeoverOption(s)}</option>
            {/each}
          </select>
          <small class="muted">{TAKEOVER_HELP}</small>
        </label>
      {/if}
    </Panel>
  {/if}

  <Panel title="Paired devices">
    {#snippet actions()}
      <Button onclick={refresh}>Refresh</Button>
    {/snippet}
    {#if status && status.sync.paired > 0}
      <p class="summary" class:warn={pillOf(status).tone === "warn"}>
        <strong>{pillOf(status).text}</strong>
        {#if status.sync.reason}<br />{status.sync.reason}{/if}
      </p>
      <p class="muted">
        Last change received: {ago(status.sync.last_in_ms, now)} · sent: {ago(status.sync.last_out_ms, now)}
      </p>
    {/if}
    <ul class="list">
      {#each status?.paired ?? [] as p (p.device)}
        <li>
          <span>
            <span class="dot" class:on={p.connected} aria-hidden="true"></span>
            <strong>{p.name}</strong>
            <RoleBar held={rolesHeldBy(status, p.device)} unchecked={uncheckedRoles(status)} />
            <small class="role" class:presenting={p.presenting}>{deviceRunsLine(status, p.device)}</small>
            <small class="muted">{p.connected ? "connected" : "not connected"}{p.problem ? ` (${p.problem})` : ""}{p.address ? ` · ${p.address}` : ""} · key {p.fingerprint}</small>
          </span>
          <span class="row">
            <Button onclick={() => team({ Unpair: { device: p.device } })}>Unpair</Button>
          </span>
        </li>
      {:else}
        <li class="muted">No paired devices yet.</li>
      {/each}
    </ul>
  </Panel>

  <Panel title="Remotes">
    {#snippet actions()}
      <Button onclick={() => (connecting = true)}>Connect a phone or tablet</Button>
    {/snippet}
    <p class="muted">Phones and tablets that run the slides and the sound from their browser. Revoke one to disconnect it at once.</p>
    <ul class="list">
      {#each remotes?.sessions ?? [] as r (r.id)}
        <li>
          <span>
            <span class="dot" class:on={r.connected} aria-hidden="true"></span>
            <strong>{r.name}</strong>
            <small class="muted">{r.connected ? "connected now" : `last seen ${ago(r.last_seen_ms, now)}`} · paired {new Date(r.paired_ms).toLocaleDateString()}</small>
          </span>
          <span class="row">
            <Button onclick={() => remote({ Revoke: { id: r.id } })}>Revoke</Button>
          </span>
        </li>
      {:else}
        <li class="muted">No remotes yet.</li>
      {/each}
    </ul>
  </Panel>
</div>

{#if connecting}
  <ConnectRemote onclose={() => (connecting = false)} />
{/if}

<style>
  .ready {
    margin-bottom: 0.5rem;
  }
  .link {
    font: inherit;
    font-size: 0.85rem;
    color: var(--muted);
    background: none;
    border: none;
    padding: 0.25rem 0.5rem;
    text-decoration: underline;
    cursor: pointer;
  }
  .link:hover {
    color: var(--text);
  }
  .small {
    font-size: 0.8rem;
    margin-top: 0.75rem;
  }
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
  .takeover {
    margin-top: 1rem;
  }
  select {
    font: inherit;
    padding: 0.4rem 0.6rem;
    border-radius: 6px;
    border: 1px solid var(--line);
    background: var(--raised);
    color: var(--text);
  }
  .role {
    margin-right: 0.4rem;
    color: var(--muted);
  }
  .role.presenting {
    color: var(--accent);
  }
  .summary {
    margin: 0 0 0.3rem;
  }
  .summary.warn {
    color: var(--warn);
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
