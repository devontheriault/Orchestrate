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
   * project, but a reap still deletes branches and conversations, so the
   * button opens a confirmation under the heading that says so, and marks the
   * rows it would take, before anything is deleted.
   */
  let confirmingReap = $state(false);
  let cancelReapEl: HTMLButtonElement | undefined = $state();

  const bulkReap = $derived(store.bulkReaps[projectId]);

  // Focus lands on Cancel, so a stray Enter backs out rather than deletes.
  $effect(() => {
    if (confirmingReap) cancelReapEl?.focus();
  });

  function reapAll() {
    confirmingReap = false;
    store.reapDelivered(projectId);
  }

  function plural(n: number, word: string): string {
    return `${n} ${word}${n === 1 ? "" : "s"}`;
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
        {#if bucket === "delivered" && !bulkReap}
          <button
            class="reap-all"
            class:open={confirmingReap}
            onclick={() => (confirmingReap = !confirmingReap)}
            title="Reap all delivered agents"
            aria-label="Reap all delivered agents"
            aria-expanded={confirmingReap}
          >
            <svg viewBox="0 0 16 16" width="13" height="13" aria-hidden="true">
              <path
                d="M2.75 4.25h10.5M6.25 4.25V2.75h3.5v1.5M4 4.25l.6 8.2a1 1 0 0 0 1 .93h4.8a1 1 0 0 0 1-.93l.6-8.2M6.75 6.75v4M9.25 6.75v4"
                fill="none"
                stroke="currentColor"
                stroke-width="1.3"
                stroke-linecap="round"
                stroke-linejoin="round"
              />
            </svg>
          </button>
        {/if}
      </div>
      {#if bucket === "delivered" && bulkReap}
        <div class="reap-panel" role="status">
          <p class="reap-title">
            {#if bulkReap.total === 0}
              Checking for unmerged work…
            {:else}
              Reaping {Math.min(bulkReap.done + 1, bulkReap.total)} of {bulkReap.total}…
            {/if}
          </p>
          <div class="reap-progress">
            <div
              class="reap-progress-fill"
              style={`width: ${bulkReap.total ? (bulkReap.done / bulkReap.total) * 100 : 0}%`}
            ></div>
          </div>
        </div>
      {:else if bucket === "delivered" && confirmingReap}
        <div
          class="reap-panel"
          role="alertdialog"
          aria-labelledby={`reap-title-${projectId}`}
          tabindex="-1"
          onkeydown={(e) => e.key === "Escape" && (confirmingReap = false)}
        >
          <p class="reap-title" id={`reap-title-${projectId}`}>
            Reap {plural(group.length, "agent")}?
          </p>
          <p class="reap-sub">
            Merged work will be deleted.
          </p>
          <div class="reap-actions">
            <button
              bind:this={cancelReapEl}
              class="btn btn-ghost btn-sm"
              onclick={() => (confirmingReap = false)}>Cancel</button
            >
            <button class="btn btn-danger btn-sm" onclick={reapAll}>Reap</button>
          </div>
        </div>
      {/if}
      {#each group as a (a.id)}
        <div
          class="agent"
          class:selected={store.selectedAgentId === a.id}
          class:doomed={bucket === "delivered" && (confirmingReap || !!bulkReap)}
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

  /* Pulled back in by its own size so the Delivered heading sits at the same
     height as the others. */
  .reap-all {
    display: grid;
    place-items: center;
    width: 1.3rem;
    height: 1.3rem;
    margin: -0.3rem 0 -0.3rem auto;
    padding: 0;
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--fg-muted);
    opacity: 0.7;
    cursor: pointer;
    transition:
      background var(--transition-fast),
      color var(--transition-fast),
      opacity var(--transition-fast);
  }

  .group:hover .reap-all,
  .reap-all:focus-visible {
    opacity: 1;
  }

  .reap-all:hover,
  .reap-all.open {
    opacity: 1;
    background: var(--danger-soft-bg);
    color: var(--danger-text);
  }

  .reap-all:focus-visible {
    outline: none;
    box-shadow: var(--focus-ring);
  }

  .reap-panel {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    margin: var(--space-3) var(--space-2) var(--space-4);
    padding: var(--space-5);
    border: 1px solid var(--danger-soft-border);
    border-radius: var(--radius-md);
    background: var(--danger-soft-bg);
    outline: none;
  }

  .reap-panel p {
    margin: 0;
  }

  .reap-title {
    font-size: var(--text-sm);
    font-weight: var(--weight-semibold);
    color: var(--fg);
    font-variant-numeric: tabular-nums;
  }

  .reap-sub {
    font-size: var(--text-xs);
    line-height: var(--leading-snug);
    color: var(--fg-muted);
  }

  .reap-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin-top: var(--space-1);
  }

  .reap-progress {
    height: 3px;
    border-radius: var(--radius-pill);
    background: var(--danger-soft-border);
    overflow: hidden;
  }

  .reap-progress-fill {
    height: 100%;
    background: var(--danger);
    transition: width var(--duration-slow) var(--ease);
  }

  .agent {
    position: relative;
    display: flex;
    align-items: center;
    gap: var(--space-3);
    /* Three one-line rows tall, so the name has room to wrap and say what the
       agent is doing rather than trailing off after a few words. */
    min-height: calc(3 * (var(--text-sm) * var(--leading-tight) + 0.6rem));
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

  /* The rows a reap-all would take, marked while it is being confirmed and
     while it runs, so it is plain which ones go. */
  .agent.doomed .prompt,
  .agent.doomed .age {
    color: var(--fg-muted);
  }

  .agent.doomed::before {
    background: var(--danger-soft-border);
  }

  .prompt {
    flex: 1;
    min-width: 0;
    font-size: var(--text-sm);
    line-height: var(--leading-tight);
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    overflow: hidden;
    overflow-wrap: anywhere;
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
