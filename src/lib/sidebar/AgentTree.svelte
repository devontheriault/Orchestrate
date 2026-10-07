<script lang="ts">
  /**
   * One project's agents, grouped by state — the list that opens underneath a
   * project. Used inline in the sidebar, and inside the rail's flyout when the
   * sidebar is too narrow to nest anything.
   */
  import { store } from "$lib/state/store.svelte";
  import { models } from "$lib/state/models.svelte";
  import { hosts } from "$lib/state/hosts.svelte";
  import type { Agent } from "$lib/api";
  import { tagColor } from "$lib/theme/tags";
  import { rowDetail } from "./agentRow";
  import { glide, measure } from "./flip";
  import { members, teams, working, type Team } from "./teams";
  import MailIcon from "$lib/mail/MailIcon.svelte";

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

  /**
   * A Lead's Helpers sit in its Bucket, under it, so the team is never split
   * across headings (ADR 0019). While any of it works, all of it is Running.
   */
  function teamBucket(team: Team): Bucket {
    return working(team) ? "running" : bucketOf(team.lead);
  }

  const agents = $derived(store.agentsForProject(projectId));

  /**
   * Whether this project's agents are on more than one Host. Only then does
   * each row say which: a project on one machine needs no label.
   */
  const spansHosts = $derived(new Set(agents.map((a) => a.host)).size > 1);

  const grouped = $derived.by(() => {
    const groups: Partial<Record<Bucket, Team[]>> = {};
    for (const t of teams(agents)) (groups[teamBucket(t)] ??= []).push(t);
    return bucketOrder
      .filter((b) => groups[b]?.length)
      .map((b) => ({ bucket: b, teams: groups[b]!, agents: groups[b]!.flatMap(members) }));
  });

  /**
   * An agent changing heading glides there instead of jumping, and the rows it
   * passes make room. Only a change of heading does it: a spawn, a discard or a
   * fold just lands, so the list isn't forever in motion. Positions are taken
   * before the list re-renders and played back after.
   */
  let treeEl: HTMLDivElement | undefined = $state();
  let lastBucket = new Map<string, Bucket>();
  let pending: { before: Map<string, DOMRect>; moved: Set<string> } | null = null;

  $effect.pre(() => {
    const next = new Map<string, Bucket>();
    for (const g of grouped) for (const a of g.agents) next.set(a.id, g.bucket);
    const moved = new Set<string>();
    for (const [id, b] of next) {
      const was = lastBucket.get(id);
      if (was && was !== b) moved.add(`agent:${id}`);
    }
    lastBucket = next;
    if (moved.size && treeEl) pending = { before: measure(treeEl), moved };
  });

  $effect(() => {
    void grouped;
    if (!pending || !treeEl) return;
    glide(treeEl, pending.before, pending.moved);
    pending = null;
  });

  /**
   * Every bucket folds. Delivered starts folded: it is the pile you are done
   * with, so it shouldn't push live work off the screen. The rest start open.
   */
  function isOpen(bucket: Bucket): boolean {
    return store.openBuckets[projectId]?.[bucket] ?? bucket !== "delivered";
  }

  function toggle(bucket: Bucket) {
    const opening = !isOpen(bucket);
    (store.openBuckets[projectId] ??= {})[bucket] = opening;
    // Clear only shows while Delivered is open, so folding it backs out.
    if (bucket === "delivered" && !opening) confirmingClear = false;
  }

  /**
   * The rows a bucket shows. A folded one still shows the agent that is on
   * screen, so the selection is never hidden and folding is never refused. A
   * Helper on screen brings its Lead, which says whose it is.
   */
  function shown(bucket: Bucket, group: Team[]): Team[] {
    if (isOpen(bucket)) return group;
    const on = (a: Agent) => a.id === store.selectedAgentId;
    return group
      .filter((t) => members(t).some(on))
      .map((t) => ({ lead: t.lead, helpers: t.helpers.filter(on) }));
  }

  /** Whether a Lead's Helpers show under it. Every team starts open. */
  function teamOpen(team: Team): boolean {
    return !store.foldedTeams[team.lead.id];
  }

  function toggleTeam(team: Team) {
    store.foldedTeams[team.lead.id] = teamOpen(team);
  }

  /**
   * The Helpers a team shows. A folded one still shows the Helper on screen,
   * as a folded bucket does, so the selection is never hidden.
   */
  function shownHelpers(team: Team): Agent[] {
    if (teamOpen(team)) return team.helpers;
    return team.helpers.filter((a) => a.id === store.selectedAgentId);
  }

  function detail(a: Agent): string {
    return rowDetail(a, store.eventsByAgent.get(a.id) ?? [], {
      delivered: store.isDelivered(a),
      holdingWork: store.isHoldingWork(a.id),
      modelName: a.model ? models.name(a.model) : "",
    });
  }

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
   * project, but clearing still deletes branches and conversations, so the
   * button opens a confirmation under the heading that says so, and marks the
   * rows it would take, before anything is deleted.
   */
  let confirmingClear = $state(false);
  let cancelClearEl: HTMLButtonElement | undefined = $state();

  const bulkClear = $derived(store.bulkClears[projectId]);

  // Focus lands on Cancel, so a stray Enter backs out rather than deletes.
  $effect(() => {
    if (confirmingClear) cancelClearEl?.focus();
  });

  function clearAll() {
    confirmingClear = false;
    store.clearDelivered(projectId);
  }

  function plural(n: number, word: string): string {
    return `${n} ${word}${n === 1 ? "" : "s"}`;
  }
</script>

<div class="tree" class:flat bind:this={treeEl}>
  <button
    class="new"
    class:active={store.drafting && store.selectedProjectId === projectId}
    onclick={spawn}
    title="Spawn a new agent on this project (n)"
  >
    <span class="plus" aria-hidden="true">+</span> New agent
  </button>

  {#each grouped as { bucket, teams: groupTeams, agents: group } (bucket)}
    {@const open = isOpen(bucket)}
    <div class="group">
      <div
        class="group-label"
        class:clearable={bucket === "delivered" && open && !bulkClear}
        data-flip={`bucket:${bucket}`}
      >
        <button
          class="fold"
          aria-expanded={open}
          onclick={() => toggle(bucket)}
          title={`${open ? "Hide" : "Show"} ${bucketLabel[bucket].toLowerCase()} agents`}
        >
          {bucketLabel[bucket]}
          <span class="count">({group.length})</span>
          <span class="rule" aria-hidden="true"></span>
          <span class="chevron" class:open aria-hidden="true">›</span>
        </button>
        {#if bucket === "delivered" && open}
          {#if !bulkClear}
            <button
              class="clear-all"
              class:open={confirmingClear}
              onclick={() => (confirmingClear = !confirmingClear)}
              title="Clear all delivered agents"
              aria-label="Clear all delivered agents"
              aria-expanded={confirmingClear}
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
        {/if}
      </div>
      {#if bucket === "delivered" && bulkClear}
        <div class="clear-panel" role="status">
          <p class="clear-title">
            {#if bulkClear.total === 0}
              Checking for unmerged work…
            {:else}
              Clearing {Math.min(bulkClear.done + 1, bulkClear.total)} of {bulkClear.total}…
            {/if}
          </p>
          <div class="clear-progress">
            <div
              class="clear-progress-fill"
              style={`width: ${bulkClear.total ? (bulkClear.done / bulkClear.total) * 100 : 0}%`}
            ></div>
          </div>
        </div>
      {:else if bucket === "delivered" && confirmingClear}
        <div
          class="clear-panel"
          role="alertdialog"
          aria-labelledby={`clear-title-${projectId}`}
          tabindex="-1"
          onkeydown={(e) => e.key === "Escape" && (confirmingClear = false)}
        >
          <p class="clear-title" id={`clear-title-${projectId}`}>
            Clear {plural(group.filter((a) => store.isDelivered(a)).length, "agent")}?
          </p>
          <p class="clear-sub">
            Merged work will be deleted.
          </p>
          <div class="clear-actions">
            <button
              bind:this={cancelClearEl}
              class="btn btn-ghost btn-sm"
              onclick={() => (confirmingClear = false)}>Cancel</button
            >
            <button class="btn btn-danger btn-sm" onclick={clearAll}>Clear</button>
          </div>
        </div>
      {/if}
      {#each shown(bucket, groupTeams) as team (team.lead.id)}
        {@render row(team.lead, bucket)}
        {#if team.helpers.length}
          {@const teamIsOpen = teamOpen(team)}
          {@const busy = team.helpers.filter((a) => a.state === "running").length}
          <div class="helpers">
            <!-- Only in an open bucket: a folded one lists just the agent on
                 screen, so a count here would be of the wrong thing. -->
            {#if open}
              <button
                class="team-fold"
                aria-expanded={teamIsOpen}
                onclick={() => toggleTeam(team)}
                title={`${teamIsOpen ? "Hide" : "Show"} ${store.agentName(team.lead)}'s helpers`}
              >
                <span class="chevron" class:open={teamIsOpen} aria-hidden="true">›</span>
                {plural(team.helpers.length, "helper")}
                {#if !teamIsOpen && busy}
                  <span class="busy">· {busy} running</span>
                {/if}
              </button>
            {/if}
            {#each shownHelpers(team) as helper (helper.id)}
              {@render row(helper, bucket)}
            {/each}
          </div>
        {/if}
      {/each}
    </div>
  {/each}
</div>

{#snippet row(a: Agent, bucket: Bucket)}
  <div
    class="agent"
    data-flip={`agent:${a.id}`}
    class:selected={store.selectedAgentId === a.id}
    class:running={a.state === "running"}
    class:doomed={bucket === "delivered" &&
      store.isDelivered(a) &&
      (confirmingClear || !!bulkClear)}
    class:away={!hosts.reachable(a.host)}
    onclick={() => pick(a.id)}
    role="button"
    tabindex="0"
    onkeydown={(e) => e.key === "Enter" && pick(a.id)}
    title={`${a.task.prompt}\n\n${a.id}${a.model ? ` · ${models.name(a.model)}` : ""}`}
  >
    <span class="name">
      {#if tagColor(a.color)}<span
          class="tag"
          style:background={tagColor(a.color)}
          title={`Tagged ${a.color}`}
        ></span>{/if}{#if a.read_mail}<span
          class="read-mail"
          title="Handed mail: it only reads and suggests"
          ><MailIcon name="mail" /></span
        >{/if}{store.agentName(a)}{#if spansHosts}<span class="host"
          >{hosts.label(a.host)}</span
        >{/if}
    </span>
    {#if a.state === "running"}
      <span class="age live" title="Working for {runTime(a)}">{runTime(a)}</span>
    {:else}
      <span class="age" title="Spawned {relTime(a.spawned_at)} ago"
        >{relTime(a.spawned_at)}</span
      >
    {/if}
    <!-- An agent on a Host that can't be reached is shown as it was last
         seen; the line under it says why it isn't live. -->
    <span class="detail" class:failed={a.state === "failed"}
      >{hosts.problem(a.host) ?? detail(a)}</span
    >
    {#if a.state === "running" && a.model}
      <span class="model">{models.name(a.model)}</span>
    {/if}
  </div>
{/snippet}

<style>
  .tree {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    /* Just a sliver in from each side, so the rows use the sidebar's width. */
    padding: 0.15rem var(--space-3) 0.6rem;
  }

  .tree.flat {
    padding: var(--space-3);
  }

  /* Laid out like an agent row, so it reads as the list's first entry rather
     than a control bolted on top. */
  .new {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    width: 100%;
    padding: 0.4rem 0.5rem;
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--fg-muted);
    font-size: var(--text-sm);
    text-align: left;
    cursor: pointer;
  }

  /* While the blank page is open, this row reads as the thing that's showing —
     but only as a faint wash, like the selected project above it. */
  .new.active {
    background: color-mix(in srgb, var(--selected) 35%, transparent);
    color: var(--accent);
  }

  .new:hover,
  .new.active:hover {
    background: var(--hover);
    color: var(--accent);
  }

  .plus {
    width: 0.5rem;
    font-size: var(--text-lg);
    line-height: var(--leading-none);
    text-align: center;
  }

  .group + .group {
    margin-top: var(--space-2);
  }

  /* One cell: the fold spans the row, and the clear button is laid over it
     just left of the chevron. */
  .group-label {
    display: grid;
    align-items: center;
    padding: 0.35rem 0.5rem 0.2rem;
    font-size: var(--text-3xs);
    font-weight: var(--weight-semibold);
    color: var(--fg-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .count {
    font-variant-numeric: tabular-nums;
    opacity: 0.7;
  }

  /* Runs from the heading to the chevron, so each bucket reads as its own
     section. */
  .rule {
    flex: 1;
    min-width: 0;
    height: var(--border-width);
    background: var(--border);
  }

  /* Each heading is its own toggle; it keeps the label's look. It spans the
     rule too, so the chevron sits at the line's right end. */
  .fold {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: 0;
    border: none;
    background: transparent;
    color: inherit;
    font: inherit;
    letter-spacing: inherit;
    text-transform: inherit;
    cursor: pointer;
  }

  .group-label > * {
    grid-area: 1 / 1;
  }

  /* Stops short of the clear button, a gap clear of it. */
  .clearable .rule {
    margin-right: calc(1.3rem + var(--space-2));
  }

  .fold:hover {
    color: var(--fg);
  }

  .chevron {
    width: 0.7rem;
    font-size: var(--text-sm);
    line-height: var(--leading-none);
    text-align: center;
    transition: transform var(--transition-fast);
  }

  .chevron.open {
    transform: rotate(90deg);
  }

  /* Pulled back in by its own size so the Delivered heading sits at the same
     height as the others, and in from the right by the chevron and a gap. */
  .clear-all {
    display: grid;
    place-items: center;
    justify-self: end;
    width: 1.3rem;
    height: 1.3rem;
    margin: -0.3rem calc(0.7rem + var(--space-2)) -0.3rem 0;
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

  .group:hover .clear-all,
  .clear-all:focus-visible {
    opacity: 1;
  }

  .clear-all:hover,
  .clear-all.open {
    opacity: 1;
    background: var(--danger-soft-bg);
    color: var(--danger-text);
  }

  .clear-all:focus-visible {
    outline: none;
    box-shadow: var(--focus-ring);
  }

  .clear-panel {
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

  .clear-panel p {
    margin: 0;
  }

  .clear-title {
    font-size: var(--text-sm);
    font-weight: var(--weight-semibold);
    color: var(--fg);
    font-variant-numeric: tabular-nums;
  }

  .clear-sub {
    font-size: var(--text-xs);
    line-height: var(--leading-snug);
    color: var(--fg-muted);
  }

  .clear-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin-top: var(--space-1);
  }

  .clear-progress {
    height: 3px;
    border-radius: var(--radius-pill);
    background: var(--danger-soft-border);
    overflow: hidden;
  }

  .clear-progress-fill {
    height: 100%;
    background: var(--danger);
    transition: width var(--duration-slow) var(--ease);
  }

  /* Name and age across the top; what it's doing underneath. */
  .agent {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    column-gap: var(--space-3);
    row-gap: 0.2rem;
    align-items: baseline;
    padding: 0.5rem 0.5rem 0.55rem;
    border-radius: var(--radius-sm);
    cursor: pointer;
    color: var(--fg);
  }

  .agent:hover {
    background: var(--hover);
  }

  /* On a touch screen, rows a thumb can land on. */
  :global(html[data-frame="mobile"]) .agent {
    padding-block: 0.75rem 0.8rem;
  }

  :global(html[data-frame="mobile"]) .new {
    padding-block: 0.7rem;
  }

  /* A running agent's row has a band of its accent sweeping through it, so the
     work shows across the whole row. It sits under the text and over the
     hover/selected tint, so both still show. */
  .agent.running {
    position: relative;
    isolation: isolate;
    overflow: hidden;
  }

  .agent.running::before {
    content: "";
    position: absolute;
    inset: 0;
    z-index: -1;
    background: linear-gradient(
      90deg,
      transparent,
      var(--running-bg) 35%,
      var(--running-bg) 65%,
      transparent
    );
    transform: translateX(-100%);
    animation: running-sweep 2.8s var(--ease) infinite;
  }

  /* Most of the cycle is the sweep; the rest is a rest, so the rows don't
     read as a constant flicker. */
  @keyframes running-sweep {
    70%,
    100% {
      transform: translateX(100%);
    }
  }

  /* With motion off the band would stop off the row's edge; a flat tint says
     "running" instead. */
  @media (prefers-reduced-motion: reduce) {
    .agent.running::before {
      transform: none;
      background: var(--running-bg);
    }
  }

  /* Just a faint wash of the accent: the project's own edge already carries
     it at full strength. Tinted rather than grey so it can't be mistaken for
     a hovered row, even though it is no heavier than one. */
  .agent.selected {
    background: color-mix(in srgb, var(--accent) 10%, transparent);
  }

  .agent.selected:hover {
    background: color-mix(in srgb, var(--accent) 15%, transparent);
  }

  /* The rows a clear would take, marked while it is being confirmed and
     while it runs, so it is plain which ones go. */
  .agent.doomed .name,
  .agent.doomed .age,
  .agent.doomed .detail {
    color: var(--fg-muted);
  }

  .name {
    font-size: var(--text-sm);
    font-weight: var(--weight-semibold);
    line-height: var(--leading-tight);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* The colour the user tagged it with (`/color`): a dot leading the name, so
     a glance down the list finds it without the whole row changing colour. */
  .tag {
    display: inline-block;
    width: 0.5rem;
    height: 0.5rem;
    margin-right: var(--space-2);
    border-radius: var(--radius-circle);
    vertical-align: 0.05em;
  }

  /* Held to the row's right edge: on a running row the column is as wide as
     the model name under it, and the time would otherwise sit in from the
     edge the other rows' times end on. */
  .age {
    justify-self: end;
    font-size: var(--text-2xs);
    color: var(--fg-muted);
    font-variant-numeric: tabular-nums;
  }

  /* A clock that's ticking should look like it belongs to the running band. */
  .age.live {
    color: var(--running);
  }

  .detail {
    grid-column: 1 / -1;
    font-size: var(--text-2xs);
    line-height: var(--leading-tight);
    color: var(--fg-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* A working agent's line says what it is doing rather than what it runs on,
     so the model moves here, under the clock. */
  .detail:has(+ .model) {
    grid-column: 1;
  }

  .model {
    font-size: var(--text-2xs);
    line-height: var(--leading-tight);
    color: var(--fg-muted);
    white-space: nowrap;
    justify-self: end;
  }

  .detail.failed {
    color: color-mix(in srgb, var(--failed) 80%, var(--fg-muted));
  }

  /* A Lead's Helpers (ADR 0019), set in under it behind a thread, so the team
     reads as one piece of work. */
  .helpers {
    margin-left: var(--space-5);
    padding-left: var(--space-1);
    border-left: var(--border-width) solid var(--border);
  }

  /* Folds a Lead's Helpers. Small and quiet, like the bucket headings, so it
     reads as part of the thread rather than as another agent. */
  .team-fold {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    padding: 0.2rem 0.5rem;
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--fg-muted);
    font-size: var(--text-2xs);
    font-variant-numeric: tabular-nums;
    text-align: left;
    cursor: pointer;
  }

  .team-fold:hover {
    background: var(--hover);
    color: var(--fg);
  }

  .team-fold:focus-visible {
    outline: none;
    box-shadow: var(--focus-ring);
  }

  :global(html[data-frame="mobile"]) .team-fold {
    padding-block: 0.5rem;
  }

  .busy {
    color: var(--running);
  }

  /* Handed mail (ADR 0018): it only reads and suggests, for good. Leads the
     name like the tag, so a long name's ellipsis can't hide it. */
  .read-mail {
    margin-right: var(--space-2);
    color: var(--fg-muted);
    vertical-align: -0.1em;
  }

  /* Which machine it's on, after the name, when the project spans several. */
  .host {
    margin-left: var(--space-3);
    font-size: var(--text-2xs);
    font-weight: var(--weight-normal);
    color: var(--fg-muted);
  }

  /* On a Host that can't be reached: still listed — hiding it would read as a
     Discard — but plainly not live. */
  .agent.away {
    opacity: 0.55;
  }
</style>
