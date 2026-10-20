<!-- Setup › OBS (T89): where OBS's WebSocket server is, its password, and
     whether Lampstand is connected. The password is sent to the core once
     and never comes back: an empty field keeps the saved one. -->
<script lang="ts">
  import { untrack } from "svelte";
  import { live, obs, refreshObs, report } from "../../lib/state.svelte";
  import { notice, SERVER_OFF_STEPS } from "../../lib/obs";
  import type { ObsConnection } from "../../lib/api";
  import Button from "../../lib/kit/Button.svelte";
  import Panel from "../../lib/kit/Panel.svelte";
  import StatusPill from "../../lib/kit/StatusPill.svelte";
  import ObsNotice from "../operate/ObsNotice.svelte";

  let host = $state("localhost");
  let port = $state(4455);
  let password = $state("");
  let busy = $state(false);

  // Fill the form once from the saved settings.
  let filled = false;
  $effect(() => {
    const view = live.obs;
    if (!view || filled) return;
    filled = true;
    untrack(() => {
      host = view.host;
      port = view.port;
    });
  });
  refreshObs();

  async function save(clearPassword = false) {
    busy = true;
    try {
      await obs({
        Configure: {
          host: host.trim(),
          port: Number(port),
          password: clearPassword ? "" : password === "" ? null : password,
        },
      });
      password = "";
    } catch (e) {
      report(e);
    }
    busy = false;
  }

  function describe(c: ObsConnection): { tone: "ready" | "warn" | "neutral"; text: string } {
    if (c === "Connecting") return { tone: "neutral", text: "Connecting…" };
    if (c === "PasswordRequired") return { tone: "warn", text: "Needs the password" };
    if (c === "WrongPassword") return { tone: "warn", text: "Wrong password" };
    if ("Connected" in c) return { tone: "ready", text: `Connected · OBS ${c.Connected.obs_version}` };
    return { tone: "warn", text: "Not connected" };
  }

  // On this screen OBS always matters, whatever the outputs.
  let view = $derived(live.obs ? { ...live.obs, relevant: true } : null);
  // The warning's own steps replace the panel below.
  let stepsShown = $derived((notice(view)?.steps.length ?? 0) > 0);
  let shown = $derived(live.obs ? describe(live.obs.connection) : null);
  let reason = $derived(
    live.obs && typeof live.obs.connection === "object" && "Disconnected" in live.obs.connection
      ? live.obs.connection.Disconnected.reason
      : null,
  );
</script>

<div class="page">
  <h1>OBS</h1>
  <p class="muted intro">
    Lampstand sends lyrics and slides to OBS as a browser source (Setup › Screens). Connect it to OBS's WebSocket
    server too, and Present shows whether you're live, and warns you when OBS stops.
  </p>

  <div class="stack">
    <Panel title="Connection" description="OBS 28 or later. Use localhost when OBS runs on this computer.">
      {#snippet actions()}
        {#if shown}<StatusPill tone={shown.tone}>{shown.text}</StatusPill>{/if}
      {/snippet}
      <form
        class="form"
        onsubmit={(e) => {
          e.preventDefault();
          save();
        }}
      >
        <label>
          <span>Computer</span>
          <input type="text" bind:value={host} autocomplete="off" spellcheck="false" placeholder="localhost" />
        </label>
        <label class="port">
          <span>Port</span>
          <input type="number" bind:value={port} min="1" max="65535" />
        </label>
        <label>
          <span>Password</span>
          <input
            type="password"
            bind:value={password}
            autocomplete="off"
            placeholder={live.obs?.has_password ? "Saved · type to change" : "From Show Connect Info in OBS"}
          />
        </label>
        <div class="buttons">
          <Button variant="primary" onclick={() => save()} disabled={busy || !host.trim()}>
            {busy ? "Testing…" : "Save and test"}
          </Button>
          {#if live.obs?.has_password}
            <Button variant="ghost" onclick={() => save(true)} disabled={busy}>Remove password</Button>
          {/if}
        </div>
      </form>
      {#if reason}<p class="muted reason">{reason}</p>{/if}
      <div class="notice"><ObsNotice {view} /></div>
    </Panel>

    {#if !stepsShown}
      <Panel title="Turn on OBS's WebSocket server" description="It's off when OBS is first installed.">
        <ol class="steps">
          {#each SERVER_OFF_STEPS as step (step)}<li>{step}</li>{/each}
        </ol>
        <p class="muted small">Lampstand never changes OBS's own settings.</p>
      </Panel>
    {/if}
  </div>
</div>

<style>
  .page {
    max-width: 64rem;
    margin: 0 auto;
    padding: 2rem;
  }
  h1 {
    font-size: 1.3rem;
    font-weight: 600;
    letter-spacing: -0.01em;
  }
  .intro {
    margin: 0.3rem 0 1.5rem;
    max-width: 40rem;
    font-size: 0.875rem;
    line-height: 1.6;
  }
  .stack {
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }
  .form {
    display: grid;
    grid-template-columns: minmax(10rem, 1fr) 7rem minmax(10rem, 1fr);
    gap: 0.75rem;
    align-items: end;
    margin-top: 0.75rem;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    font-size: 0.8125rem;
    color: var(--muted);
  }
  .buttons {
    grid-column: 1 / -1;
    display: flex;
    gap: 0.5rem;
  }
  .reason {
    margin-top: 0.75rem;
    font-size: 0.8125rem;
  }
  .notice:not(:empty) {
    margin-top: 0.75rem;
  }
  .steps {
    margin: 0.75rem 0 0;
    padding-left: 1.2rem;
    font-size: 0.875rem;
    line-height: 1.7;
  }
  .small {
    margin-top: 0.5rem;
    font-size: 0.8125rem;
  }
  @media (max-width: 720px) {
    .form {
      grid-template-columns: 1fr;
    }
  }
</style>
