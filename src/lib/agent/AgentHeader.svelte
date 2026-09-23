<script lang="ts">
  import { store } from "$lib/state/store.svelte";

  /** The blank page for an agent that hasn't been spawned yet. */
  const drafting = $derived(store.drafting && !store.selectedAgent);

  /**
   * Reap deletes the worktree, the branch, and with it the agent's session —
   * everything not merged out is gone. It used to happen only as part of an
   * explicit Stop; now that Stop preserves work, Reap is the one destructive
   * button in the app, so it takes two clicks.
   */
  let reapArmed = $state(false);
  let disarm: ReturnType<typeof setTimeout> | undefined;

  function armReap() {
    reapArmed = true;
    clearTimeout(disarm);
    disarm = setTimeout(() => (reapArmed = false), 4000);
  }

  function reap() {
    clearTimeout(disarm);
    reapArmed = false;
    store.reapAgent(store.selectedAgent!.id);
  }

  // Never leave the trigger armed across a change of agent.
  $effect(() => {
    store.selectedAgentId;
    clearTimeout(disarm);
    reapArmed = false;
  });
</script>

<!-- The heading moved to the window bar above, which is painted like this
     pane; what's left here are the pane's own controls. -->
<header>
  {#if store.selectedAgent}
    <div class="tabs" role="tablist">
      <button
        role="tab"
        aria-selected={store.detailTab === "output"}
        class:active={store.detailTab === "output"}
        onclick={() => store.showTab("output")}>Output</button
      >
      <button
        role="tab"
        aria-selected={store.detailTab === "diff"}
        class:active={store.detailTab === "diff"}
        onclick={() => store.showTab("diff")}
      >
        Diff
        {#if store.review.diff?.uncommitted}
          <span class="dirty-dot" title="uncommitted work"></span>
        {/if}
      </button>
    </div>
    <!-- Stop lives in the composer's send slot while the agent runs; the
         header keeps only the destructive action, once it's stopped. -->
    {#if store.selectedAgent.state !== "running"}
      <div class="head-right">
        <button
          class="reap"
          class:armed={reapArmed}
          onclick={() => (reapArmed ? reap() : armReap())}
          onblur={() => (reapArmed = false)}
          title="Delete this agent's worktree and branch — uncommitted work and its conversation go with them"
        >
          {reapArmed ? "Reap for good?" : "Reap"}
        </button>
      </div>
    {/if}
  {:else}
    <div class="head-right">
      <button class="cancel" onclick={() => store.cancelDraft()}>Cancel</button>
    </div>
  {/if}
</header>

<style>
  header {
    padding: var(--pad-y) var(--pad-x);
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem 1rem;
    min-height: 2.7rem;
  }

  .cancel:hover { border-color: var(--accent); color: var(--accent); }

  .head-right {
    display: flex;
    flex: none;
    align-items: center;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 0.4rem 0.5rem;
    margin-left: auto;
  }

  .head-right button {
    flex: none;
    white-space: nowrap;
    border-radius: var(--radius-sm);
    border: 1px solid var(--border);
    background: var(--surface);
    padding: 0.35rem 0.85rem;
    font-size: var(--text-md);
    font-family: inherit;
    color: var(--fg);
    cursor: pointer;
  }

  .tabs {
    display: inline-flex;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    overflow: hidden;
  }

  .tabs button {
    border: none;
    border-radius: 0;
    background: transparent;
    padding: 0.32rem 0.8rem;
    font-size: var(--text-md);
    color: var(--fg-muted);
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    gap: var(--space-3);
  }

  .tabs button:hover { color: var(--fg); }

  .tabs button.active {
    background: var(--selected);
    color: var(--fg);
    font-weight: var(--weight-medium);
  }

  .dirty-dot {
    width: 6px;
    height: 6px;
    border-radius: var(--radius-circle);
    background: var(--warning);
  }

  .reap:hover { border-color: var(--accent); color: var(--accent); }

  .reap.armed,
  .reap.armed:hover {
    border-color: var(--danger);
    background: var(--danger);
    color: var(--on-danger);
    font-weight: var(--weight-medium);
  }
</style>
