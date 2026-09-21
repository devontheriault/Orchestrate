<script lang="ts">
  /**
   * One project's agents, grouped by state — the list that opens underneath a
   * project. Used inline in the sidebar, and inside the rail's flyout when the
   * sidebar is too narrow to nest anything.
   */
  import { store } from "./store.svelte";
  import type { Agent } from "./api";

  let {
    projectId,
    /** Called after the user picks or spawns — the flyout closes on it. */
    onpick,
    /** Flush left, for the flyout: nothing above it to indent under. */
    flat = false,
  }: { projectId: string; onpick?: () => void; flat?: boolean } = $props();

  const stateOrder = ["running", "failed", "orphaned", "completed", "stopped"];
  const stateLabel: Record<Agent["state"], string> = {
    running: "Running",
    completed: "Completed",
    failed: "Failed",
    stopped: "Stopped",
    orphaned: "Orphaned",
  };

  const agents = $derived(store.agentsForProject(projectId));

  const grouped = $derived.by(() => {
    const groups: Record<string, Agent[]> = {};
    for (const a of agents) (groups[a.state] ??= []).push(a);
    return stateOrder
      .filter((s) => groups[s]?.length)
      .map((s) => ({ state: s as Agent["state"], agents: groups[s] }));
  });

  function firstLine(p: string): string {
    return p.split("\n")[0] ?? "";
  }

  function relTime(iso: string): string {
    const then = new Date(iso).getTime();
    const s = Math.max(0, Math.floor((Date.now() - then) / 1000));
    if (s < 60) return `${s}s`;
    if (s < 3600) return `${Math.floor(s / 60)}m`;
    if (s < 86400) return `${Math.floor(s / 3600)}h`;
    return `${Math.floor(s / 86400)}d`;
  }

  function pick(id: string) {
    store.selectAgent(id);
    onpick?.();
  }

  function spawn() {
    store.startDraft(projectId);
    onpick?.();
  }
</script>

<div class="tree" class:flat>
  <button
    class="new"
    class:active={store.drafting && store.selectedProjectId === projectId}
    onclick={spawn}
    title="Spawn a new agent on this project (n)"
  >
    <span class="plus">+</span> New agent
  </button>

  {#if agents.length === 0}
    <p class="empty">No agents yet.</p>
  {:else}
    {#each grouped as { state, agents: group } (state)}
      <div class="group">
        <div class="group-label">
          <span class={`dot dot-${state}`}></span>
          {stateLabel[state]}
          <span class="count">{group.length}</span>
        </div>
        {#each group as a (a.id)}
          <div
            class="agent"
            class:selected={store.selectedAgentId === a.id}
            onclick={() => pick(a.id)}
            role="button"
            tabindex="0"
            onkeydown={(e) => e.key === "Enter" && pick(a.id)}
            title={`${a.task.prompt}\n\n${a.id}${a.model ? ` · ${store.modelName(a.model)}` : ""}`}
          >
            <span class="prompt">{firstLine(a.task.prompt)}</span>
            <span class="age">{relTime(a.spawned_at)}</span>
          </div>
        {/each}
      </div>
    {/each}
  {/if}
</div>

<style>
  .tree {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
    /* Indented to sit under the project name, beside the guide line below. */
    padding: 0.15rem 0.4rem 0.45rem calc(var(--pad-x) + 0.75rem);
  }

  /* The line that ties the agents to the project they belong to. */
  .tree::before {
    content: "";
    position: absolute;
    left: calc(var(--pad-x) + 0.25rem);
    top: 0.1rem;
    bottom: 0.5rem;
    width: 1px;
    background: var(--border);
  }

  .tree.flat {
    padding: 0.4rem;
  }

  .tree.flat::before {
    display: none;
  }

  .new {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    width: 100%;
    padding: 0.3rem 0.45rem;
    margin-bottom: 0.1rem;
    border: 1px dashed var(--border);
    border-radius: 5px;
    background: transparent;
    color: var(--fg-muted);
    font-size: 0.78rem;
    text-align: left;
    cursor: pointer;
  }

  .new:hover {
    border-color: var(--accent);
    color: var(--accent);
    border-style: solid;
  }

  /* While the blank page is open, this row reads as the thing that's showing. */
  .new.active {
    border-style: solid;
    border-color: var(--accent);
    background: var(--selected);
    color: var(--accent);
  }

  .plus {
    font-size: 0.95rem;
    line-height: 1;
  }

  .empty {
    margin: 0.2rem 0 0.1rem;
    padding: 0 0.45rem;
    font-size: 0.74rem;
    color: var(--fg-muted);
  }

  .group + .group {
    margin-top: 0.3rem;
  }

  .group-label {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.25rem 0.45rem 0.15rem;
    font-size: 0.66rem;
    font-weight: 600;
    color: var(--fg-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .count {
    font-variant-numeric: tabular-nums;
    opacity: 0.8;
  }

  .dot {
    width: 6px;
    height: 6px;
    flex: none;
    border-radius: 50%;
    display: inline-block;
  }
  .dot-running {
    background: var(--running);
    box-shadow: 0 0 0 3px var(--running-bg);
    animation: pulse 2s infinite;
  }
  .dot-completed { background: #3b82f6; }
  .dot-failed { background: #ef4444; }
  .dot-orphaned { background: var(--attention); }
  .dot-stopped { background: #9ca3af; }

  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.5; }
  }

  @media (prefers-reduced-motion: reduce) {
    .dot-running { animation: none; }
  }

  .agent {
    display: flex;
    align-items: baseline;
    gap: 0.4rem;
    padding: 0.3rem 0.45rem;
    border-radius: 5px;
    cursor: pointer;
    color: var(--fg);
  }

  .agent:hover {
    background: var(--hover);
  }

  .agent.selected {
    background: var(--selected);
    box-shadow: inset 2px 0 0 var(--accent);
  }

  .agent.selected .prompt {
    font-weight: 500;
  }

  .prompt {
    flex: 1;
    min-width: 0;
    font-size: 0.8rem;
    line-height: 1.3;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .age {
    flex: none;
    font-size: 0.68rem;
    color: var(--fg-muted);
    font-variant-numeric: tabular-nums;
  }
</style>
