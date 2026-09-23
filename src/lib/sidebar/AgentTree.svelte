<script lang="ts">
  /**
   * One project's agents, grouped by state — the list that opens underneath a
   * project. Used inline in the sidebar, and inside the rail's flyout when the
   * sidebar is too narrow to nest anything.
   */
  import { store } from "$lib/state/store.svelte";
  import { models } from "$lib/state/models.svelte";
  import type { Agent } from "$lib/api";

  let {
    projectId,
    /** Called after the user picks or spawns — the flyout closes on it. */
    onpick,
    /** Flush left, for the flyout: nothing above it to indent under. */
    flat = false,
  }: { projectId: string; onpick?: () => void; flat?: boolean } = $props();

  /**
   * The heading an agent sits under. Usually its state, but an agent whose work
   * has reached the project and who has written nothing since is bucketed as
   * Delivered instead: where the work went says more to a reader than how the
   * last turn happened to end. A bucket is a view of an agent, not a property
   * of one — a Merge stays a record.
   */
  type Bucket = Agent["state"] | "delivered";

  // Delivered sits last: it is the pile you are done with.
  const bucketOrder: Bucket[] = [
    "running",
    "failed",
    "orphaned",
    "completed",
    "stopped",
    "delivered",
  ];
  const bucketLabel: Record<Bucket, string> = {
    running: "Running",
    completed: "Completed",
    failed: "Failed",
    stopped: "Stopped",
    orphaned: "Orphaned",
    delivered: "Delivered",
  };

  /**
   * Checked in order, first match winning:
   *
   * 1. Running — an agent that is working is the thing you most need to see,
   *    wherever its last turn's work ended up, so it outranks the record.
   * 2. No merge record — it sits under its own state.
   * 3. Merged but holding work the project hasn't got, because it was resumed
   *    and left something dirty or committed again — back under its own state,
   *    which for an agent that exited cleanly is Completed. Delivered is the
   *    pile you are done with, and an agent still carrying work isn't that.
   * 4. Otherwise Delivered.
   *
   * An agent goes back to its own state rather than always to Completed: one
   * that merged, was resumed, and then failed belongs under Failed.
   */
  function bucketOf(a: Agent): Bucket {
    if (a.state === "running") return "running";
    return store.isDelivered(a) ? "delivered" : a.state;
  }

  const agents = $derived(store.agentsForProject(projectId));

  const grouped = $derived.by(() => {
    const groups: Partial<Record<Bucket, Agent[]>> = {};
    for (const a of agents) (groups[bucketOf(a)] ??= []).push(a);
    return bucketOrder
      .filter((b) => groups[b]?.length)
      .map((b) => ({ bucket: b, agents: groups[b]! }));
  });

  const anyRunning = $derived(agents.some((a) => a.state === "running"));

  // The times on these rows move on their own: by the second while something is
  // working, so a running agent's clock is live, and slowly otherwise, so a
  // finished agent's age doesn't sit frozen at whatever it read when the list
  // last re-rendered.
  let now = $state(Date.now());
  $effect(() => {
    const id = setInterval(() => (now = Date.now()), anyRunning ? 1000 : 30_000);
    return () => clearInterval(id);
  });

  function secondsSince(iso: string): number {
    return Math.max(0, Math.floor((now - new Date(iso).getTime()) / 1000));
  }

  /** How long the running turn has been going: "12s", "3m 04s", "1h 12m". */
  function runTime(a: Agent): string {
    const total = secondsSince(a.turn_started_at ?? a.spawned_at);
    const h = Math.floor(total / 3600);
    const m = Math.floor((total % 3600) / 60);
    const s = total % 60;
    if (h > 0) return `${h}h ${String(m).padStart(2, "0")}m`;
    if (m > 0) return `${m}m ${String(s).padStart(2, "0")}s`;
    return `${s}s`;
  }

  function relTime(iso: string): string {
    const s = secondsSince(iso);
    if (s < 60) return `${s}s`;
    if (s < 3600) return `${Math.floor(s / 60)}m`;
    if (s < 86400) return `${Math.floor(s / 3600)}h`;
    return `${Math.floor(s / 86400)}d`;
  }

  function pick(id: string) {
    // Not selectAgent: an agent under another project should bring that
    // project with it, or the header and composer keep pointing at the old one.
    store.showAgent(id);
    onpick?.();
  }

  function spawn() {
    store.startDraft(projectId);
    onpick?.();
  }

  /**
   * Clearing out the Delivered pile in one go. Their work is already in the
   * project, but a reap still deletes branches and conversations, so like the
   * header's Reap it takes two clicks.
   */
  let reapAllArmed = $state(false);
  let reapingAll = $state(false);
  let disarm: ReturnType<typeof setTimeout> | undefined;

  function armReapAll() {
    reapAllArmed = true;
    clearTimeout(disarm);
    disarm = setTimeout(() => (reapAllArmed = false), 4000);
  }

  async function reapAll() {
    clearTimeout(disarm);
    reapAllArmed = false;
    reapingAll = true;
    try {
      await store.reapDelivered(projectId);
    } finally {
      reapingAll = false;
    }
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

  {#each grouped as { bucket, agents: group } (bucket)}
    <div class="group">
      <div class="group-label">
        <span class={`status-dot status-${bucket}`}></span>
        {bucketLabel[bucket]}
        <span class="count">{group.length}</span>
        {#if bucket === "delivered"}
          <button
            class="reap-all"
            class:armed={reapAllArmed}
            disabled={reapingAll}
            onclick={() => (reapAllArmed ? reapAll() : armReapAll())}
            onblur={() => (reapAllArmed = false)}
            title="Delete every delivered agent's worktree and branch — their work is already merged, but their conversations go with them"
          >
            {#if reapingAll}
              Reaping…
            {:else if reapAllArmed}
              Reap {group.length} for good?
            {:else}
              Reap all
            {/if}
          </button>
        {/if}
      </div>
      {#each group as a (a.id)}
        <div
          class="agent"
          class:selected={store.selectedAgentId === a.id}
          onclick={() => pick(a.id)}
          role="button"
          tabindex="0"
          onkeydown={(e) => e.key === "Enter" && pick(a.id)}
          title={`${a.task.prompt}\n\n${a.id}${a.model ? ` · ${models.name(a.model)}` : ""}`}
        >
          <span class="prompt">{store.agentName(a)}</span>
          {#if a.state === "running"}
            <span class="age live" title="Working for {runTime(a)}">{runTime(a)}</span>
          {:else}
            <span class="age" title="Spawned {relTime(a.spawned_at)} ago"
              >{relTime(a.spawned_at)}</span
            >
          {/if}
        </div>
      {/each}
    </div>
  {/each}
</div>

<style>
  .tree {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
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
    padding: var(--space-3);
  }

  .tree.flat::before {
    display: none;
  }

  .new {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    width: 100%;
    padding: 0.3rem 0.45rem;
    margin-bottom: var(--space-1);
    border: 1px dashed var(--border);
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--fg-muted);
    font-size: var(--text-sm);
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
    font-size: var(--text-xl);
    line-height: var(--leading-none);
  }

  .group + .group {
    margin-top: var(--space-2);
  }

  .group-label {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: 0.25rem 0.45rem 0.15rem;
    font-size: var(--text-3xs);
    font-weight: var(--weight-semibold);
    color: var(--fg-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .count {
    font-variant-numeric: tabular-nums;
    opacity: 0.8;
  }

  /* Pulled back in by its own padding and border so the Delivered heading sits
     at the same height as the others. */
  .reap-all {
    margin: calc(-0.1rem - 1px) 0 calc(-0.1rem - 1px) auto;
    padding: 0.1rem 0.35rem;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--fg-muted);
    font: inherit;
    letter-spacing: inherit;
    text-transform: inherit;
    cursor: pointer;
  }

  .reap-all:hover:not(:disabled) {
    border-color: var(--accent);
    color: var(--accent);
  }

  .reap-all:disabled {
    cursor: default;
    opacity: 0.7;
  }

  .reap-all.armed,
  .reap-all.armed:hover {
    border-color: var(--danger);
    background: var(--danger);
    color: var(--on-danger);
  }

  .agent {
    position: relative;
    display: flex;
    align-items: baseline;
    gap: var(--space-3);
    padding: 0.3rem 0.45rem;
    border-radius: var(--radius-sm);
    cursor: pointer;
    color: var(--fg);
  }

  /* The elbow off the guide line: a short tick that points at this row, so an
     agent reads as hanging from its project rather than floating beside it. */
  .agent::before {
    content: "";
    position: absolute;
    left: -0.5rem;
    top: 50%;
    width: 0.5rem;
    height: 1px;
    background: var(--border);
  }

  /* No guide line in the flyout, so nothing for a tick to come off. */
  .tree.flat .agent::before {
    display: none;
  }

  .agent:hover {
    background: var(--hover);
  }

  .agent:hover::before {
    background: var(--accent);
  }

  /* The selected row already carries the accent on its left edge — the tick
     stays grey so the blue reads in one place. Listed after the hover rule so
     it holds while the pointer is over the row. */
  .agent.selected::before {
    background: color-mix(in srgb, var(--fg-muted) 55%, var(--border));
  }

  .agent.selected {
    background: var(--selected);
    box-shadow: inset 2px 0 0 var(--accent);
  }

  .agent.selected .prompt {
    font-weight: var(--weight-medium);
  }

  .prompt {
    flex: 1;
    min-width: 0;
    font-size: var(--text-sm);
    line-height: var(--leading-tight);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .age {
    flex: none;
    font-size: var(--text-2xs);
    color: var(--fg-muted);
    font-variant-numeric: tabular-nums;
  }

  /* A clock that's ticking should look like it belongs to the running dot. */
  .age.live {
    color: var(--running);
  }
</style>
