/**
 * The app's state: its Projects and Agents, what's selected, and the
 * lifecycle verbs — Spawn, Resume, Stop, Discard — the UI calls on them.
 *
 * Three slices hang off it, each in its own file because each is its own
 * concern that happens to need the Agents and the selection:
 *
 * - `review` — the selected Agent's diff, and the Commit / Merge / Resolve
 *   taken from the Diff tab (`review.svelte.ts`).
 * - `queue` — messages lined up behind a working Agent (`queue.svelte.ts`).
 * - `prefs` — the Model, Effort, Mode and options a new Agent opens on
 *   (`prefs.svelte.ts`).
 *
 * The model list is its own store (`models.svelte.ts`): it needs nothing here.
 */

import {
  api,
  events,
  type Agent,
  type AgentEvent,
  type AgentOptions,
  type HostStatus,
  type Project,
} from "$lib/api";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { models } from "./models.svelte";
import { TurnPrefs } from "./prefs.svelte";
import { Queue } from "./queue.svelte";
import { Review } from "./review.svelte";

/** What a project's agents are doing right now, summarised for the sidebar. */
export type ProjectActivity = {
  running: number;
  /** Agents that exited needing a human look: failed or orphaned. */
  attention: number;
  /** Agents that finished cleanly — what a quiet project has to show for itself. */
  completed: number;
  /** The most recently spawned running agent, for a "what's it doing" line. */
  latestRunning: Agent | null;
};

const NO_ACTIVITY: ProjectActivity = {
  running: 0,
  attention: 0,
  completed: 0,
  latestRunning: null,
};

/** Identity of an event, for reconciling a replayed log with live arrivals. */
function eventKey(e: AgentEvent): string {
  return `${e.ts}:${JSON.stringify(e.event)}`;
}

/**
 * The log is written before an event is emitted, so a replay is a superset of
 * what arrived live — except for anything emitted after the read. Keep the
 * log's order and append only the live events it doesn't already contain.
 */
function mergeEvents(logged: AgentEvent[], live: AgentEvent[]): AgentEvent[] {
  if (live.length === 0) return logged;
  const seen = new Set(logged.map(eventKey));
  return [...logged, ...live.filter((e) => !seen.has(eventKey(e)))];
}

/**
 * Where the user was, stashed across a reload. sessionStorage is scoped to this
 * webview session, so it survives Ctrl+Alt+R and is gone on the next launch.
 */
const RESUME_KEY = "cw:resume-selection";

type Selection = { project: string | null; agent: string | null };

function takeResumeSelection(): Selection | null {
  try {
    const raw = sessionStorage.getItem(RESUME_KEY);
    if (!raw) return null;
    sessionStorage.removeItem(RESUME_KEY);
    return JSON.parse(raw) as Selection;
  } catch {
    return null;
  }
}

export class AppStore {
  projects = $state<Project[]>([]);
  agents = $state<Agent[]>([]);
  orphans = $state<Agent[]>([]);
  eventsByAgent = $state<Record<string, AgentEvent[]>>({});

  selectedProjectId = $state<string | null>(null);
  selectedAgentId = $state<string | null>(null);

  /**
   * Which projects are showing their agents in the sidebar. A project opens
   * when it's selected and closes when its row is clicked again, so the tree
   * only ever holds the lists the user asked to see.
   */
  expandedProjects = $state<Record<string, boolean>>({});

  /**
   * Buckets the user has opened or folded in the sidebar, by project and then
   * by bucket. Unset means the bucket's default. Kept here rather than in the
   * list, so collapsing the project or opening the rail's flyout shows the
   * tree as the user left it.
   */
  openBuckets = $state<Record<string, Record<string, boolean>>>({});

  /** Which body the detail pane shows for the selected agent. */
  detailTab = $state<"output" | "diff">("output");

  /**
   * Merged agents whose worktree still holds work the project doesn't have.
   * Read from git rather than from the agent record, because a merge record
   * only says what once happened — it can't know the agent was resumed and
   * wrote more since. Empty until the first read lands, so an agent reads as
   * Delivered until git says otherwise rather than flickering on startup.
   */
  holdingWork = $state<string[]>([]);

  /**
   * Clears of Delivered in progress, by project. Kept here rather than in the
   * list that starts one, so collapsing the project or closing the flyout mid-run
   * doesn't lose the progress. `total` is 0 until git has said which are safe.
   */
  bulkClears = $state<Record<string, { done: number; total: number }>>({});

  /** A follow-up prompt is in flight for the selected agent. */
  sending = $state<boolean>(false);

  /**
   * A new agent being composed. The detail pane shows a blank output page with
   * an empty transcript and the composer waiting for the opening prompt —
   * the same surface the agent's own output will fill, rather than a dialog
   * over the top of it. Mutually exclusive with having an agent selected.
   */
  drafting = $state<boolean>(false);

  /** The opening prompt is in flight; the draft page is waiting on a spawn. */
  spawning = $state<boolean>(false);

  orphanBannerDismissed = $state<boolean>(false);
  error = $state<string | null>(null);

  /** Where this window stands with the Host that owns the agents. */
  host = $state<HostStatus>({ state: "connecting", error: null });

  /** The Host last connected to, to tell a reconnection from a successor. */
  private hostInstance: string | null = null;

  /** Whether `refresh` has ever succeeded. */
  private loaded = false;

  /** The selected agent's diff, and what the Diff tab does with it. */
  readonly review = new Review(this);

  /** Messages waiting behind each agent. */
  readonly queue = new Queue(this);

  /** The picks a new agent opens on. */
  readonly prefs = new TurnPrefs(this);

  /** Agents whose on-disk log has been replayed into `eventsByAgent`. */
  private hydrated = new Set<string>();

  /** Agents with a Turn already being started, so a drain can't double-send. */
  private turnsInFlight = new Set<string>();

  private unlisteners: UnlistenFn[] = [];
  private started = false;

  /** Every project's agents, newest first — the sidebar reads one list per row. */
  agentsByProject = $derived.by(() => {
    const map = new Map<string, Agent[]>();
    for (const a of this.agents) {
      const list = map.get(a.project_id);
      if (list) list.push(a);
      else map.set(a.project_id, [a]);
    }
    for (const list of map.values()) {
      list.sort((x, y) => Date.parse(y.spawned_at) - Date.parse(x.spawned_at));
    }
    return map;
  });

  agentsForProject(projectId: string): Agent[] {
    return this.agentsByProject.get(projectId) ?? [];
  }

  /** Whether a merged agent has work the project hasn't got. See `holdingWork`. */
  isHoldingWork(agentId: string): boolean {
    return this.holdingWork.includes(agentId);
  }

  /**
   * Whether an agent sits in the sidebar's Delivered bucket: merged, not
   * working, and holding nothing the project hasn't got.
   */
  isDelivered(a: Agent): boolean {
    return a.state !== "running" && !!a.merged_at && !this.isHoldingWork(a.id);
  }

  activityByProject = $derived.by(() => {
    const map = new Map<string, ProjectActivity>();
    for (const a of this.agents) {
      const entry = map.get(a.project_id) ?? { ...NO_ACTIVITY };
      if (a.state === "running") {
        entry.running += 1;
        const newer =
          !entry.latestRunning ||
          Date.parse(a.spawned_at) >= Date.parse(entry.latestRunning.spawned_at);
        if (newer) entry.latestRunning = a;
      } else if (a.state === "failed" || a.state === "orphaned") {
        entry.attention += 1;
      } else if (a.state === "completed") {
        entry.completed += 1;
      }
      map.set(a.project_id, entry);
    }
    return map;
  });

  activityFor(projectId: string): ProjectActivity {
    return this.activityByProject.get(projectId) ?? NO_ACTIVITY;
  }

  selectedAgent = $derived(
    this.selectedAgentId
      ? this.agents.find((a) => a.id === this.selectedAgentId) ?? null
      : null,
  );

  eventsForSelected = $derived(
    this.selectedAgentId ? this.eventsByAgent[this.selectedAgentId] ?? [] : [],
  );

  /**
   * Whether the selected agent's conversation can be picked back up. Needs a
   * session to resume and a worktree to run in — the backend has the last word
   * on the worktree, since only it can see the disk.
   */
  canContinue = $derived.by(() => {
    const a = this.selectedAgent;
    return !!a && a.state !== "running" && !!a.session_id;
  });

  async start() {
    if (this.started) return;
    this.started = true;

    this.unlisteners.push(
      await events.onAgentEvent(({ agent_id, event }) => {
        if (!this.eventsByAgent[agent_id]) this.eventsByAgent[agent_id] = [];
        this.eventsByAgent[agent_id].push(event);
      }),
    );

    this.unlisteners.push(await events.onHostStatus((s) => this.onHostStatus(s)));
    api.hostStatus().then((s) => this.onHostStatus(s)).catch(() => {});

    this.unlisteners.push(
      await events.onAgentStateChanged((agent) => {
        const i = this.agents.findIndex((a) => a.id === agent.id);
        if (i >= 0) this.agents[i] = agent;
        else this.agents.push(agent);
        // An agent that just exited has a final diff worth showing.
        if (agent.id === this.selectedAgentId && this.detailTab === "diff") {
          this.review.load();
        }
        // A Turn that ended cleanly is the moment anything queued behind it
        // becomes sendable.
        if (agent.state === "completed") this.queue.drain(agent.id);
        // A merged agent's worktree only changes because the agent ran, so a
        // turn ending is the one moment its bucket can have moved. That is why
        // nothing here polls git on a timer.
        if (agent.state !== "running" && agent.merged_at) this.loadHoldingWork();
      }),
    );

    // Not awaited: the model list only fills a picker, and blocking the first
    // paint on a network round-trip would be a poor trade.
    models.load();
    await this.refresh();
  }

  /**
   * What to call an agent: the Title the user gave it, else the one Claude
   * wrote for it, or — until that lands, and on agents from before titles
   * existed — the first line of the prompt it was given.
   */
  agentName(a: Agent): string {
    const name = a.user_title?.trim() || a.title?.trim();
    if (name) return name;
    return a.task.prompt.split("\n")[0] ?? "";
  }

  async stop() {
    for (const un of this.unlisteners) un();
    this.unlisteners = [];
    this.started = false;
  }

  async refresh() {
    try {
      this.error = null;
      [this.projects, this.agents, this.orphans, this.holdingWork] =
        await Promise.all([
          api.listProjects(),
          api.listAgents(),
          api.startupOrphans(),
          api.agentsHoldingWork(),
        ]);
      this.loaded = true;
      this.queue.prune();
      this.applyInitialSelection();
    } catch (e) {
      this.error = String(e);
    }
  }

  /**
   * Follow the window's connection to the Host. Coming back to it, or to the
   * one that replaced it, means re-reading everything: whatever the agents did
   * in between reached the logs but not this window.
   */
  private onHostStatus(status: HostStatus) {
    const was = this.host;
    this.host = status;
    if (status.state !== "connected") return;
    if (was.state === "connected" && was.instance === status.instance) return;

    const previous = this.hostInstance;
    this.hostInstance = status.instance;
    // A new Host adopted its own Orphans; the banner is about those now.
    if (previous !== null && previous !== status.instance) {
      this.orphanBannerDismissed = false;
    }
    // The first connection is the one `start` is already loading from,
    // unless that load gave up waiting for it.
    if (previous !== null || !this.loaded) this.resync();
  }

  private async resync() {
    this.hydrated.clear();
    await this.refresh();
    if (this.selectedAgentId) this.hydrateEvents(this.selectedAgentId);
  }

  /**
   * Re-ask git which merged agents are holding work. Swallows failures: the
   * previous answer is a better sidebar than an error banner over a reading.
   */
  async loadHoldingWork() {
    try {
      this.holdingWork = await api.agentsHoldingWork();
    } catch {
      // Keep the last answer.
    }
  }

  /**
   * Pick what to show on a fresh load: whatever a reload asked us to resume,
   * else the first project.
   */
  private applyInitialSelection() {
    if (this.selectedProjectId) return;
    const resume = takeResumeSelection();
    const project =
      resume?.project && this.projects.some((p) => p.id === resume.project)
        ? resume.project
        : this.projects[0]?.id ?? null;
    this.selectedProjectId = project;
    // The first thing on screen should already be showing its agents.
    if (project) this.expandedProjects[project] = true;

    const resumable =
      resume?.agent &&
      this.agents.some((a) => a.id === resume.agent && a.project_id === project);
    if (resumable) this.selectAgent(resume.agent);
  }

  /** Stash the current selection, then reload the webview. */
  reload() {
    try {
      sessionStorage.setItem(
        RESUME_KEY,
        JSON.stringify({
          project: this.selectedProjectId,
          agent: this.selectedAgentId,
        } satisfies Selection),
      );
    } catch {
      // Not worth blocking the reload over.
    }
    window.location.reload();
  }

  async addProject(name: string, path: string, setUp: boolean) {
    try {
      const p = await api.addProject(name, path, setUp);
      this.projects.push(p);
      this.selectProject(p.id);
    } catch (e) {
      this.error = String(e);
    }
  }

  async removeProject(id: string) {
    try {
      await api.removeProject(id);
      this.projects = this.projects.filter((p) => p.id !== id);
      delete this.expandedProjects[id];
      delete this.openBuckets[id];
      if (this.selectedProjectId === id) {
        this.selectedProjectId = null;
        this.selectAgent(null);
        const next = this.projects[0]?.id;
        if (next) this.selectProject(next);
      }
    } catch (e) {
      this.error = String(e);
    }
  }

  /**
   * Select a project and open its agents underneath it. Moving to a *different*
   * project drops the agent selection — the detail pane belongs to the project
   * on screen — while re-selecting the one already showing leaves it alone, so
   * re-opening a closed row doesn't cost the user the agent they were reading.
   */
  selectProject(id: string) {
    const moved = this.selectedProjectId !== id;
    this.selectedProjectId = id;
    this.expandedProjects[id] = true;
    if (moved) this.selectAgent(null);
  }

  /** What a click on a project row does: open it, or close the open one. */
  toggleProject(id: string) {
    if (this.selectedProjectId === id && this.expandedProjects[id]) {
      this.expandedProjects[id] = false;
      return;
    }
    this.selectProject(id);
  }

  selectAgent(id: string | null) {
    const wasDrafting = this.drafting;
    this.drafting = false;
    if (id === this.selectedAgentId && !wasDrafting) return;
    this.selectedAgentId = id;
    this.review.clear();
    // Opening an Agent starts on its output; the diff is something you go look
    // for, so it shouldn't carry over from whichever Agent was open before.
    this.detailTab = "output";
    if (id) this.hydrateEvents(id);
  }

  /**
   * Open the blank page for a new agent: nothing selected, output tab, and a
   * composer holding the opening prompt until the user sends it. `projectId`
   * says which project it will belong to; without one, the selected project.
   */
  startDraft(projectId?: string) {
    const project = projectId ?? this.selectedProjectId;
    if (!project) return;
    this.selectedProjectId = project;
    this.expandedProjects[project] = true;
    this.selectedAgentId = null;
    this.review.clear();
    this.detailTab = "output";
    this.drafting = true;
  }

  cancelDraft() {
    this.drafting = false;
  }

  /**
   * Fill an Agent's output pane from its log on disk. Only the events streamed
   * to this window are held in memory, so after a reload — or for an Agent that
   * ran before this window opened — the pane would otherwise start empty.
   */
  async hydrateEvents(id: string) {
    if (this.hydrated.has(id)) return;
    this.hydrated.add(id);
    try {
      const logged = await api.agentEvents(id);
      this.eventsByAgent[id] = mergeEvents(logged, this.eventsByAgent[id] ?? []);
    } catch (e) {
      // Leave it unhydrated so re-selecting the Agent tries again.
      this.hydrated.delete(id);
      this.error = String(e);
    }
  }

  showTab(tab: "output" | "diff") {
    this.detailTab = tab;
    // Always re-read on entry: the worktree may have moved since last time.
    if (tab === "diff" && !this.review.loading) this.review.load();
  }

  /**
   * Bring an Agent on screen wherever it lives: switch to its Project, then
   * select it. False if there is no such Agent any more — its log can outlive
   * it, so a caller working from the logs can be holding a Discarded id.
   */
  showAgent(id: string): boolean {
    const agent = this.agents.find((a) => a.id === id);
    if (!agent) return false;
    if (this.projects.some((p) => p.id === agent.project_id)) {
      this.selectedProjectId = agent.project_id;
      this.expandedProjects[agent.project_id] = true;
    }
    this.selectAgent(id);
    return true;
  }

  jumpToFirstOrphan() {
    if (this.orphans.length === 0) return;
    const first = this.orphans[0];
    if (this.projects.some((p) => p.id === first.project_id)) {
      this.selectedProjectId = first.project_id;
      this.expandedProjects[first.project_id] = true;
    }
    this.selectAgent(first.id);
    this.dismissOrphans();
  }

  /** Hide the Orphan banner, and tell the backend so a reload keeps it hidden. */
  dismissOrphans() {
    this.orphanBannerDismissed = true;
    api.dismissOrphans().catch(() => {});
  }

  /**
   * Start a new agent on the selected project, with any files attached to its
   * prompt. `model` is the picker's value; empty means no `--model` at all.
   * Returns whether it started — the draft page keeps the prompt on failure so
   * the user can retry rather than retype.
   */
  async spawn(
    prompt: string,
    attachments: string[],
    model: string,
    effort: string,
    mode: string,
  ): Promise<boolean> {
    if (!this.selectedProjectId || !prompt.trim() || this.spawning) return false;
    this.spawning = true;
    this.error = null;
    try {
      const agent = await api.spawnAgent(
        this.selectedProjectId,
        prompt,
        attachments,
        model || null,
        effort || null,
        mode || null,
        this.prefs.options,
      );
      this.prefs.remember(model, effort, mode);
      this.agents.push(agent);
      this.selectAgent(agent.id);
      return true;
    } catch (e) {
      this.error = String(e);
      return false;
    } finally {
      this.spawning = false;
    }
  }

  /**
   * Send a follow-up prompt, and any files attached to it, to the selected
   * agent, putting it back to work in the worktree it already has, on `model`
   * and in `mode`. Returns whether the agent took it.
   */
  async resume(
    prompt: string,
    attachments: string[],
    model: string,
    effort: string,
    mode: string,
  ) {
    const id = this.selectedAgentId;
    if (!id || !prompt.trim() || this.sending) return false;
    return this.sendTurn(id, prompt, attachments, model, effort, mode);
  }

  /**
   * Start a Turn on a named Agent. Takes an id rather than reading the selection
   * because a queue drains whether or not its Agent is the one on screen, and
   * `sending` — a flag the composer reads — only speaks for the Agent it shows.
   */
  async sendTurn(
    id: string,
    prompt: string,
    attachments: string[],
    model: string,
    effort: string,
    mode: string,
  ): Promise<boolean> {
    if (this.turnsInFlight.has(id)) return false;
    this.turnsInFlight.add(id);
    const onScreen = id === this.selectedAgentId;
    if (onScreen) this.sending = true;
    this.error = null;
    try {
      const agent = await api.resumeAgent(
        id,
        prompt,
        attachments,
        model || null,
        effort || null,
        mode || null,
      );
      this.prefs.remember(model, effort, mode);
      const i = this.agents.findIndex((a) => a.id === agent.id);
      if (i >= 0) this.agents[i] = agent;
      // It's working again, so it's nobody's leftover any more.
      this.orphans = this.orphans.filter((o) => o.id !== id);
      return true;
    } catch (e) {
      this.error = String(e);
      return false;
    } finally {
      this.turnsInFlight.delete(id);
      // Cleared on the same condition it was set on: if the selection moved
      // mid-flight, the flag no longer speaks for the Agent on screen anyway.
      if (onScreen) this.sending = false;
    }
  }

  /**
   * Name an agent, or with null let Claude's title name it again. Returns
   * whether it took. Allowed while the agent works, as are the edits below.
   */
  renameAgent(id: string, name: string | null): Promise<boolean> {
    return this.edit(api.renameAgent(id, name));
  }

  /** Tag an agent with a colour, or untag it with null. */
  setAgentColor(id: string, color: string | null): Promise<boolean> {
    return this.edit(api.setAgentColor(id, color));
  }

  /**
   * Set how an agent's turns run from its next one on — and, since options
   * are meant to stick, what the next new agent spawns with.
   */
  async setAgentOptions(id: string, options: AgentOptions): Promise<boolean> {
    const done = await this.edit(api.setAgentOptions(id, options));
    if (done) this.prefs.rememberOptions(options);
    return done;
  }

  /**
   * Wait out an edit. The record it returns isn't put in place: the backend
   * announces the edit too, in order with the agent's other changes, and a
   * reply that lands after a Turn's end was announced would put the agent
   * back to work on screen.
   */
  private async edit(editing: Promise<Agent>): Promise<boolean> {
    try {
      await editing;
      return true;
    } catch (e) {
      this.error = String(e);
      return false;
    }
  }

  async stopAgent(id: string) {
    try {
      await api.stopAgent(id);
    } catch (e) {
      this.error = String(e);
    }
  }

  async discardAgent(id: string) {
    try {
      await api.discardAgent(id);
      this.agents = this.agents.filter((a) => a.id !== id);
      delete this.eventsByAgent[id];
      this.hydrated.delete(id);
      this.queue.clear(id);
      if (this.selectedAgentId === id) {
        this.selectedAgentId = null;
        this.review.clear();
      }
    } catch (e) {
      this.error = String(e);
    }
  }

  /**
   * Clear a project's Delivered bucket by Discarding each agent in it. Git is
   * asked afresh first rather than trusting `holdingWork`, which is only
   * re-read when a turn ends or a commit lands and reads empty before its
   * first answer — a stale "nothing held" here would destroy work. One at a
   * time, since each discard removes a worktree and deletes a branch in the
   * same repository.
   */
  async clearDelivered(projectId: string) {
    if (this.bulkClears[projectId]) return;
    this.bulkClears[projectId] = { done: 0, total: 0 };
    try {
      this.holdingWork = await api.agentsHoldingWork();
      const delivered = this.agentsForProject(projectId).filter((a) => this.isDelivered(a));
      this.bulkClears[projectId].total = delivered.length;
      for (const a of delivered) {
        await this.discardAgent(a.id);
        this.bulkClears[projectId].done++;
      }
    } catch (e) {
      this.error = String(e);
    } finally {
      delete this.bulkClears[projectId];
    }
  }
}

export const store = new AppStore();
