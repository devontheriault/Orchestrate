<script lang="ts">
  import { store } from "./store.svelte";
  import type { Agent } from "./api";

  let { onSpawn }: { onSpawn: () => void } = $props();

  const stateOrder = ["running", "failed", "orphaned", "completed", "stopped"];
  const stateLabel: Record<Agent["state"], string> = {
    running: "Running",
    completed: "Completed",
    failed: "Failed",
    stopped: "Stopped",
    orphaned: "Orphaned",
  };

  const grouped = $derived.by(() => {
    const groups: Record<string, Agent[]> = {};
    for (const a of store.agentsForSelectedProject) {
      (groups[a.state] ??= []).push(a);
    }
    return stateOrder
      .filter((s) => groups[s]?.length)
      .map((s) => ({ state: s as Agent["state"], agents: groups[s] }));
  });

  function shortPrompt(p: string): string {
    const firstLine = p.split("\n")[0] ?? "";
    return firstLine.length > 60 ? firstLine.slice(0, 57) + "…" : firstLine;
  }

  function relTime(iso: string): string {
    const then = new Date(iso).getTime();
    const now = Date.now();
    const s = Math.max(0, Math.floor((now - then) / 1000));
    if (s < 60) return `${s}s ago`;
    if (s < 3600) return `${Math.floor(s / 60)}m ago`;
    if (s < 86400) return `${Math.floor(s / 3600)}h ago`;
    return `${Math.floor(s / 86400)}d ago`;
  }
</script>

<section>
  <header>
    <span class="title">Agents</span>
    <button
      class="spawn"
      onclick={onSpawn}
      disabled={!store.selectedProjectId}
      title={store.selectedProjectId ? "Spawn new agent" : "Select a project first"}
    >
      + Spawn
    </button>
  </header>

  {#if !store.selectedProjectId}
    <div class="empty">Select a project to see its agents.</div>
  {:else if store.agentsForSelectedProject.length === 0}
    <div class="empty">
      No agents in this project yet.<br />
      <button class="spawn-empty" onclick={onSpawn}>Spawn one</button>
    </div>
  {:else}
    <div class="groups">
      {#each grouped as { state, agents } (state)}
        <div class="group">
          <div class="group-label">
            <span class={`dot dot-${state}`}></span>
            {stateLabel[state]} ({agents.length})
          </div>
          {#each agents as a (a.id)}
            <div
              class="agent"
              class:selected={store.selectedAgentId === a.id}
              onclick={() => store.selectAgent(a.id)}
              role="button"
              tabindex="0"
              onkeydown={(e) => e.key === "Enter" && store.selectAgent(a.id)}
            >
              <div class="prompt">{shortPrompt(a.task.prompt)}</div>
              <div class="meta">
                <code>{a.id}</code>
                <span class="time">{relTime(a.spawned_at)}</span>
              </div>
            </div>
          {/each}
        </div>
      {/each}
    </div>
  {/if}
</section>

<style>
  section {
    width: 320px;
    flex: 0 0 320px;
    border-right: 1px solid var(--border);
    background: var(--panel-bg);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  header {
    padding: 0.75rem 0.9rem;
    display: flex;
    align-items: center;
    justify-content: space-between;
    border-bottom: 1px solid var(--border);
  }

  .title {
    font-size: 0.78rem;
    font-weight: 600;
    color: var(--fg-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .spawn {
    background: var(--accent);
    color: white;
    border: none;
    border-radius: 5px;
    padding: 0.35rem 0.75rem;
    font-size: 0.82rem;
    font-family: inherit;
    cursor: pointer;
    font-weight: 500;
  }

  .spawn:hover:not(:disabled) {
    filter: brightness(1.1);
  }

  .spawn:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .empty {
    padding: 2rem 1rem;
    text-align: center;
    color: var(--fg-muted);
    font-size: 0.9rem;
  }

  .spawn-empty {
    margin-top: 0.75rem;
    padding: 0.4rem 0.9rem;
    background: var(--accent);
    color: white;
    border: none;
    border-radius: 5px;
    cursor: pointer;
    font-family: inherit;
    font-size: 0.85rem;
  }

  .groups {
    overflow-y: auto;
    flex: 1;
    padding: 0.5rem 0;
  }

  .group + .group {
    margin-top: 0.5rem;
  }

  .group-label {
    padding: 0.35rem 0.9rem;
    font-size: 0.72rem;
    color: var(--fg-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }

  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    display: inline-block;
  }
  .dot-running {
    background: #22c55e;
    box-shadow: 0 0 0 3px rgba(34, 197, 94, 0.15);
    animation: pulse 2s infinite;
  }
  .dot-completed { background: #3b82f6; }
  .dot-failed { background: #ef4444; }
  .dot-orphaned { background: #f59e0b; }
  .dot-stopped { background: #9ca3af; }

  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.5; }
  }

  .agent {
    padding: 0.55rem 0.9rem;
    cursor: pointer;
    border-left: 2px solid transparent;
  }

  .agent:hover {
    background: var(--hover);
  }

  .agent.selected {
    background: var(--selected);
    border-left-color: var(--accent);
  }

  .prompt {
    font-size: 0.88rem;
    line-height: 1.35;
  }

  .meta {
    margin-top: 0.2rem;
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    font-size: 0.72rem;
    color: var(--fg-muted);
  }

  code {
    font-family: ui-monospace, monospace;
    background: var(--code-bg);
    padding: 0.05em 0.35em;
    border-radius: 3px;
  }
</style>
