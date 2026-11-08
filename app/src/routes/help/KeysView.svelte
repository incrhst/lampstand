<!-- Help › Keys: every key of the Present screen, plus the screen switches. -->
<script lang="ts">
  import { KEYS, modLabel } from "../../lib/shortcuts";

  const mod = modLabel(typeof navigator === "undefined" ? "" : navigator.platform);
  const rows: [string, string][] = [...KEYS, ["O", "Control OBS, when it is connected"], [`${mod}1 · ${mod}2 · ${mod}\\`, "Plan, Present, and switch between them"]];
</script>

<div class="page">
  <h1>Keys</h1>
  <p class="muted intro">Present is made to be run from the keyboard, or from a clicker that sends the arrow keys.</p>
  <table>
    <tbody>
      {#each rows as [key, what]}
        <tr><td><kbd>{key}</kbd></td><td>{what}</td></tr>
      {/each}
    </tbody>
  </table>
</div>

<style>
  .page {
    max-width: 40rem;
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
  table {
    border-collapse: collapse;
    width: 100%;
  }
  td {
    padding: 0.55rem 0.5rem;
    border-bottom: 1px solid var(--line);
    font-size: 0.875rem;
  }
  td:first-child {
    white-space: nowrap;
    width: 14rem;
  }
  kbd {
    font: inherit;
    font-size: 0.8125rem;
    padding: 0.1rem 0.4rem;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius);
    background: var(--raised);
  }
</style>
