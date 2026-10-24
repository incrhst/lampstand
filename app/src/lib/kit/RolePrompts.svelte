<!-- What the roles need answered or noticed, across the top of every screen:
     a device asking this one to hand a role over (Allow / Not now, and
     "Always allow this device" for Operator and Stream), this device's own
     ask and its answer, and a role this device lost while away. One banner
     per role and per ask, in the spec's words (lib/roles.ts). -->
<script lang="ts">
  import Banner from "./Banner.svelte";
  import Button from "./Button.svelte";
  import { live, presenterRequest } from "../state.svelte";
  import {
    ALWAYS_ALLOW,
    NOT_NOW,
    allowBlocked,
    allowLabel,
    noticeRoles,
    replacedNotice,
    requestPrompt,
    roleChoice,
    rolesOf,
    wantsAttention,
  } from "../roles";

  let roles = $derived(rolesOf(live.team));
  let notices = $derived(noticeRoles(live.team));
  /** Answers sent and not yet settled, so a second click does nothing. */
  let answering = $state<string[]>([]);
  async function answer(key: string, req: Parameters<typeof presenterRequest>[0]) {
    if (answering.includes(key)) return;
    answering = [...answering, key];
    try {
      await presenterRequest(req);
    } finally {
      answering = answering.filter((k) => k !== key);
    }
  }
</script>

{#if wantsAttention(live.team)}
  <div class="notices">
    {#each roles as r (r.role)}
      {#if r.request}
        {@const asker = r.request}
        {@const key = `${r.role}:${asker.device}`}
        {@const why = allowBlocked(live.team, r.role)}
        {@const done = answering.includes(key)}
        <Banner tone="warn" icon="monitor">
          <span>{requestPrompt(r.role, asker.name)}</span>
          {#if why}<small id="allow-why-{r.role}">{why}</small>{/if}
          <Button
            variant="primary"
            size="small"
            disabled={!!why || done}
            describedby={why ? `allow-why-${r.role}` : undefined}
            onclick={() => answer(key, { RoleAllow: { role: r.role, device: asker.device, always: false } })}
          >
            {allowLabel(r.role)}
          </Button>
          {#if r.offers_always}
            <Button
              size="small"
              disabled={!!why || done}
              describedby={why ? `allow-why-${r.role}` : undefined}
              onclick={() => answer(key, { RoleAllow: { role: r.role, device: asker.device, always: true } })}
              title="Remember this device, so it can take this role without asking next time"
            >
              {ALWAYS_ALLOW}
            </Button>
          {/if}
          <Button size="small" variant="ghost" disabled={done} onclick={() => answer(key, { RoleDecline: { role: r.role, device: asker.device } })}>{NOT_NOW}</Button>
        </Banner>
      {/if}
    {/each}

    {#each notices as r (r.role)}
      {#if r.replaced_by}
        <Banner tone="warn">
          <span>{replacedNotice(r.role, r.replaced_by)}</span>
          <Button size="small" onclick={() => presenterRequest({ RoleDismiss: { role: r.role } })}>OK</Button>
        </Banner>
      {/if}
    {/each}

    {#each roles as r (r.role)}
      {@const choice = roleChoice(r)}
      {#if choice.kind === "waiting"}
        <Banner tone="neutral" icon="monitor">
          <span>{choice.label}</span>
          <Button size="small" onclick={() => presenterRequest({ RoleCancel: { role: r.role } })}>Cancel</Button>
        </Banner>
      {:else if choice.kind === "declined" || choice.kind === "busy"}
        <Banner tone="neutral" icon="monitor">
          <span>{choice.label}</span>
          <Button size="small" onclick={() => presenterRequest({ RoleAsk: { role: r.role } })}>{choice.again}</Button>
          <Button size="small" variant="ghost" onclick={() => presenterRequest({ RoleCancel: { role: r.role } })}>OK</Button>
        </Banner>
      {/if}
    {/each}
  </div>
{/if}

<style>
  .notices {
    display: grid;
    gap: 0.4rem;
    padding: 0.5rem 1.25rem 0;
  }
  small {
    color: var(--muted);
  }
</style>
