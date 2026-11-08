<!-- Setup › OBS: the two separate ways OBS and Lampstand work together.
     "Send to OBS" is OBS loading a picture from Lampstand (a browser
     source). "Control OBS" (T89) is Lampstand driving OBS over its
     WebSocket: where the server is, its password, and whether Lampstand is
     connected. The password is sent to the core once and never comes back:
     an empty field keeps the saved one. Either works without the other. -->
<script lang="ts">
  import { untrack } from "svelte";
  import { live, obs, putOutput, refreshObs, report } from "../../lib/state.svelte";
  import { request } from "../../lib/api";
  import { notice, isPicture, setupBlocked, setupPicture, SERVER_OFF_STEPS } from "../../lib/obs";
  import type { ObsConnection, OutputConfig } from "../../lib/api";
  import Button from "../../lib/kit/Button.svelte";
  import Panel from "../../lib/kit/Panel.svelte";
  import StatusPill from "../../lib/kit/StatusPill.svelte";
  import Icon from "../../lib/kit/Icon.svelte";
  import ObsNotice from "../operate/ObsNotice.svelte";
  import CalibrateView from "../outputs/CalibrateView.svelte";
  import ObsPicture from "./ObsPicture.svelte";

  // A picture sent to OBS is calibrated like a screen, on a page of its own.
  let calibrating: string | null = $state(null);
  let calibratingOutput = $derived(live.outputs.find((o) => o.id === calibrating) ?? null);
  async function saveSafeArea(o: OutputConfig, safe_area: OutputConfig["safe_area"], background: OutputConfig["background"]) {
    await putOutput({ ...o, safe_area, background });
    calibrating = null;
  }

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

  // OBS on another computer (#133). Nothing below shows when OBS is on this one.
  let picture = $derived(setupPicture(live.outputs.filter(isPicture)));
  let blocked = $derived(setupBlocked(live.obs, picture));
  let settingUp = $state(false);
  let setupResult: string | null = $state(null);
  /** The pin, or "" for "let Lampstand choose". */
  let pinned = $derived(live.obs?.pinned_address ?? "");
  /** This computer's addresses, and the pinned one even if it isn't here now. */
  let choices = $derived.by(() => {
    const here = live.obs?.addresses ?? [];
    return pinned && !here.includes(pinned) ? [...here, pinned] : here;
  });

  async function useAddress(address: string) {
    try {
      await obs({ UseAddress: { address: address === "" ? null : address } });
      setupResult = null;
    } catch (e) {
      report(e);
    }
  }

  async function setUpObs() {
    if (!picture) return;
    settingUp = true;
    setupResult = null;
    try {
      // The core makes the picture's address itself; only the output is named.
      setupResult = await request({ area: "Obs", request: { SetUp: { output: picture.id } } }, "Text");
    } catch (e) {
      report(e);
    }
    settingUp = false;
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

{#if calibratingOutput}
  <CalibrateView
    output={calibratingOutput}
    onsave={(area, background) => saveSafeArea(calibratingOutput!, area, background)}
    oncancel={() => (calibrating = null)}
  />
{:else}
<div class="page">
  <h1>OBS</h1>
  <p class="muted intro">
    OBS and Lampstand can work together in two separate ways. You can use either one without the other.
  </p>

  <section class="way" aria-labelledby="send-head">
    <div class="way-head">
      <Icon name="broadcast" size={20} />
      <div>
        <h2 id="send-head">Send to OBS</h2>
        <p class="flow"><strong>OBS shows Lampstand's picture.</strong> Lyrics as lower thirds, or the full screen, in a Browser source.</p>
      </div>
    </div>
    <ObsPicture oncalibrate={(id) => (calibrating = id)} />
  </section>

  <section class="way" aria-labelledby="control-head">
    <div class="way-head">
      <Icon name="remote" size={20} />
      <div>
        <h2 id="control-head">Control OBS</h2>
        <p class="flow"><strong>Lampstand tells OBS what to do.</strong> Switch scenes with the service and show whether you're live. Needs OBS's WebSocket server.</p>
      </div>
    </div>
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

      {#if live.obs?.remote}
        <Panel
          title="OBS is on another computer"
          description="OBS loads Lampstand's picture over your network, so it needs this computer's network address."
        >
          <div class="remote">
            <label class="address">
              <span>Use this address</span>
              <select value={pinned} onchange={(e) => useAddress(e.currentTarget.value)} aria-describedby="address-help">
                <option value="">Let Lampstand choose</option>
                {#each choices as a (a)}
                  <option value={a}>{a}{live.obs.addresses.includes(a) ? "" : " (not on this computer now)"}</option>
                {/each}
              </select>
            </label>
            <p id="address-help" class="muted small">
              {#if pinned}
                OBS will use {pinned} to reach this computer, even if the computer has others.
              {:else if choices.length > 1}
                This computer has {choices.length} network addresses. If OBS can't load the picture, pick the one on the same network as OBS.
              {:else}
                Pick one only if this computer has several and Lampstand chose the wrong one.
              {/if}
            </p>
            <div class="buttons">
              <Button variant="primary" onclick={setUpObs} disabled={settingUp || blocked !== null} describedby="setup-why">
                {settingUp ? "Setting up…" : "Set up OBS"}
              </Button>
            </div>
            <p id="setup-why" class="muted small">{blocked ? `Set up OBS is greyed out: ${blocked}` : ""}</p>
            <p class="muted small">
              Switches OBS to a collection called “Lampstand” (making it if it isn't there) and adds the scenes Slides, Worship and Pastor and slides to it, each showing Lampstand's picture. If the collection already has scenes with those names, the picture is added to them. Nothing in your other collections changes. Run it again after you change the address or press New key.
            </p>
            <p class="result" role="status">{setupResult ?? ""}</p>
          </div>
        </Panel>
      {/if}

      {#if !stepsShown}
        <Panel title="Turn on OBS's WebSocket server" description="It's off when OBS is first installed.">
          <ol class="steps">
            {#each SERVER_OFF_STEPS as step (step)}<li>{step}</li>{/each}
          </ol>
          <p class="muted small">Lampstand never changes OBS's own settings.</p>
        </Panel>
      {/if}
    </div>
  </section>
</div>
{/if}

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
  .way {
    margin-top: 2rem;
    padding-top: 1.5rem;
    border-top: 1px solid var(--line);
  }
  .way:first-of-type {
    margin-top: 0;
    padding-top: 0;
    border-top: 0;
  }
  .way-head {
    display: flex;
    gap: 0.75rem;
    align-items: flex-start;
    margin-bottom: 0.5rem;
    color: var(--text);
  }
  .way-head h2 {
    font-size: 1.05rem;
    font-weight: 600;
  }
  .flow {
    margin-top: 0.2rem;
    font-size: 0.875rem;
    line-height: 1.55;
    color: var(--muted);
    max-width: 44rem;
  }
  .flow strong {
    color: var(--text);
    font-weight: 600;
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
  .remote {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  .address {
    max-width: 20rem;
  }
  .result {
    font-size: 0.875rem;
    line-height: 1.5;
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
