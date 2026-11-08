<!-- Help › Help and feedback: the documentation, asking for help, reporting a
     problem and suggesting an idea. Each opens the project's page in the
     browser, with the version filled in where there is a form. -->
<script lang="ts">
  import { request } from "../../lib/api";
  import { report } from "../../lib/state.svelte";
  import type { AboutRequest } from "../../lib/api";
  import Button from "../../lib/kit/Button.svelte";
  import Panel from "../../lib/kit/Panel.svelte";

  /** Opens the project's issue form in the browser, with the version filled in. */
  async function tell(what: AboutRequest) {
    try {
      await request({ area: "About", request: what }, "Ok");
    } catch (e) {
      report(e);
    }
  }

</script>

<div class="page">
  <h1>Help and feedback</h1>
  <p class="muted intro">Guides, answers and ways to tell us what you think.</p>

  <Panel title="Documentation" description="How to plan and run a service, with short clips of each step.">
    <div class="buttons">
      <Button variant="primary" icon="file" onclick={() => tell("ReadDocs")}>Read the docs</Button>
    </div>
    <p class="muted small">Opens in your browser.</p>
  </Panel>

  <Panel title="Tell us about your church" description="An optional two-minute form: who you are and how you plan to use Lampstand, so we know who it is for and what to build next.">
    <div class="buttons">
      <Button icon="info" onclick={() => tell("TellUs")}>Open the form</Button>
    </div>
    <p class="muted small">It opens in your browser. Nothing is sent from Lampstand, and everything on it is optional.</p>
  </Panel>

  <Panel title="Help and feedback" description="Opens in your browser with your version already filled in.">
    <div class="buttons">
      <Button variant="primary" icon="info" onclick={() => tell("AskForHelp")}>Ask for help</Button>
      <Button icon="alert" onclick={() => tell("ReportProblem")}>Report a problem</Button>
      <Button icon="plus" onclick={() => tell("SuggestIdea")}>Suggest an idea</Button>
    </div>
    <p class="muted small">
      <strong>Ask for help</strong> with how to do something: other churches and the team answer in Discussions.
      <strong>Report a problem</strong> when something is broken, and <strong>Suggest an idea</strong> for something
      new. All of it is public, on GitHub (github.com/incrhst/lampstand), so please leave out passwords and OBS
      Browser source addresses, which contain a key.
    </p>
  </Panel>
</div>

<style>
  .page :global(section + section) {
    margin-top: 1rem;
  }
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
    font-size: 0.875rem;
    line-height: 1.6;
  }
  .buttons {
    display: flex;
    gap: 0.5rem;
    margin-top: 0.75rem;
  }
  .small {
    margin-top: 0.75rem;
    font-size: 0.8125rem;
  }
</style>
