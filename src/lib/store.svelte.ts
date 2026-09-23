import {
  api,
  DEFAULT_EFFORT,
  DEFAULT_MODE,
  DEFAULT_MODEL,
  events,
  modelLabel,
  type Agent,
  type AgentEvent,
  type Branches,
  type ModelInfo,
  type Project,
  type WorktreeDiff,
} from "./api";
import type { UnlistenFn } from "@tauri-apps/api/event";

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

/**
 * The model the user picked last, for any Turn — a Spawn or a Resume — so a new
 * agent's page opens on their habitual choice instead of resetting every time.
 * localStorage, not sessionStorage: a preference should outlive the window.
 *
 * The stored value is the picker's own, so the empty string is a real answer —
 * the user asking for Claude Code's default. Only a missing key means they have
 * never picked, which is the one case the fallback below gets to speak.
 */
const MODEL_KEY = "cw:preferred-model";

/** The effort the user picked last, stored on the same terms as the model. */
const EFFORT_KEY = "cw:preferred-effort";

/** The mode the user picked last, stored on the same terms as the model. */
const MODE_KEY = "cw:preferred-mode";

/**
 * Messages the user has lined up behind a working Agent, by Agent id. Written
 * to localStorage on every change so a window reload — or a relaunch after the
 * app was closed mid-Turn — doesn't quietly throw away text the user typed.
 */
const QUEUE_KEY = "cw:queues";

/**
 * One prompt waiting for an Agent to be free, with the Model, effort and mode
 * the user picked for it. The pick travels with the message rather than being
 * read at send time: it's part of what the user decided when they queued it.
 */
export type QueuedMessage = {
  /** Local id, so the UI can delete one message out of the middle. */
  id: string;
  prompt: string;
  model: string;
  effort: string;
  /** Absent on messages queued before modes existed — those run the default. */
  mode?: string;
};

/**
 * A local handle for one queued message — it only has to be unique among the
 * messages this window is holding. A counter beside the clock rather than
 * `crypto.randomUUID`, which isn't guaranteed on every webview this app runs in.
 */
let queueSeq = 0;
function queuedId(): string {
  return `q${Date.now().toString(36)}-${queueSeq++}`;
}

function readQueues(): Record<string, QueuedMessage[]> {
  try {
    const raw = localStorage.getItem(QUEUE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as Record<string, QueuedMessage[]>;
    // Anything malformed is worth less than a working queue: drop it.
    if (!parsed || typeof parsed !== "object") return {};
    return parsed;
  } catch {
    return {};
  }
}

function readStored(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

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

class AppStore {
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

  /** Which body the detail pane shows for the selected agent. */
  detailTab = $state<"output" | "diff">("output");

  /** Diff of the selected agent's worktree. Cleared when the selection moves. */
  diff = $state<WorktreeDiff | null>(null);
  diffLoading = $state<boolean>(false);
  diffError = $state<string | null>(null);
  committing = $state<boolean>(false);
  merging = $state<boolean>(false);

  /**
   * Merges that hit conflicts, by agent: where the merge was headed and which
   * files collided. Held until the user merges again or hands it to a
   * resolver — the project was left untouched, so this is the only record of
   * what went wrong.
   */
  conflicts = $state<Record<string, { target: string; files: string[] }>>({});

  /** A resolver is being spawned for the selected agent's conflict. */
  resolving = $state<boolean>(false);

  /**
   * The selected agent's project's mergeable branches. Null until asked for, and
   * again when the branch list can't be read — the picker then offers nothing
   * rather than guessing.
   */
  branches = $state<Branches | null>(null);

  /**
   * Merged agents whose worktree still holds work the project doesn't have.
   * Read from git rather than from the agent record, because a merge record
   * only says what once happened — it can't know the agent was resumed and
   * wrote more since. Empty until the first read lands, so an agent reads as
   * Delivered until git says otherwise rather than flickering on startup.
   */
  holdingWork = $state<string[]>([]);

  /** A follow-up prompt is in flight for the selected agent. */
  sending = $state<boolean>(false);

  /**
   * Messages waiting behind each Agent, oldest first — what the user said while
   * a Turn was still running. Only one `claude` may run in a Worktree at a time,
   * so a queued message is held here and sent as its own Turn when the Agent is
   * free, rather than racing the Turn already in flight.
   */
  queues = $state<Record<string, QueuedMessage[]>>(readQueues());

  /**
   * A new agent being composed. The detail pane shows a blank output page with
   * an empty transcript and the composer waiting for the opening prompt —
   * the same surface the agent's own output will fill, rather than a dialog
   * over the top of it. Mutually exclusive with having an agent selected.
   */
  drafting = $state<boolean>(false);

  /** The opening prompt is in flight; the draft page is waiting on a spawn. */
  spawning = $state<boolean>(false);

  /** The model the user picked for the most recent Turn. Null = never picked. */
  preferredModel = $state<string | null>(readStored(MODEL_KEY));

  /** The effort the user picked for the most recent Turn. Null = never picked. */
  preferredEffort = $state<string | null>(readStored(EFFORT_KEY));

  /** The mode the user picked for the most recent Turn. Null = never picked. */
  preferredMode = $state<string | null>(readStored(MODE_KEY));

  /** The models this account can run, newest first. Empty until loaded. */
  models = $state<ModelInfo[]>([]);
  modelsLoading = $state<boolean>(false);
  /**
   * Why the model list couldn't be loaded, if it couldn't. Kept out of the
   * global error banner: the pickers stay usable on their Default option, so
   * this is a note beside them rather than something to interrupt over.
   */
  modelsError = $state<string | null>(null);

  orphanBannerDismissed = $state<boolean>(false);
  error = $state<string | null>(null);

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
   * The most recently spawned Agent, for the picks it ran on. Stands
   * in for a stored preference on a profile that has none yet — the Agents on
   * disk are a record of the user's picks too, and reading them means the picker
   * is right on the first spawn after an update rather than the second.
   */
  private lastSpawned = $derived.by(() => {
    let latest: Agent | null = null;
    for (const a of this.agents) {
      if (!latest || Date.parse(a.spawned_at) >= Date.parse(latest.spawned_at)) {
        latest = a;
      }
    }
    return latest;
  });

  /** What a new agent's picker opens on: the last model the user ran on. */
  defaultSpawnModel = $derived(
    this.preferredModel ?? this.lastSpawned?.model ?? DEFAULT_MODEL,
  );

  /** The same, for the effort level beside it. */
  defaultSpawnEffort = $derived(
    this.preferredEffort ?? this.lastSpawned?.effort ?? DEFAULT_EFFORT,
  );

  /** And for the mode: what the last Turn ran as, else the YOLO default. */
  defaultSpawnMode = $derived(
    this.preferredMode ?? this.lastSpawned?.permission_mode ?? DEFAULT_MODE,
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

    this.unlisteners.push(
      await events.onAgentStateChanged((agent) => {
        const i = this.agents.findIndex((a) => a.id === agent.id);
        if (i >= 0) this.agents[i] = agent;
        else this.agents.push(agent);
        // An agent that just exited has a final diff worth showing.
        if (agent.id === this.selectedAgentId && this.detailTab === "diff") {
          this.loadDiff();
        }
        // A Turn that ended cleanly is the moment anything queued behind it
        // becomes sendable.
        if (agent.state === "completed") this.drainQueue(agent.id);
        // A merged agent's worktree only changes because the agent ran, so a
        // turn ending is the one moment its bucket can have moved. That is why
        // nothing here polls git on a timer.
        if (agent.state !== "running" && agent.merged_at) this.loadHoldingWork();
      }),
    );

    // Not awaited: the model list only fills a picker, and blocking the first
    // paint on a network round-trip would be a poor trade.
    this.loadModels();
    await this.refresh();
  }

  /** Ask the backend which models this account can run. */
  async loadModels() {
    this.modelsLoading = true;
    this.modelsError = null;
    try {
      this.models = await api.listModels();
    } catch (e) {
      this.models = [];
      this.modelsError = String(e);
    } finally {
      this.modelsLoading = false;
    }
  }

  /**
   * What to call an agent: the name Claude wrote for it, or — until that
   * lands, and on agents from before titles existed — the first line of the
   * prompt it was given.
   */
  agentName(a: Agent): string {
    const title = a.title?.trim();
    if (title) return title;
    return a.task.prompt.split("\n")[0] ?? "";
  }

  /**
   * Anthropic's name for a model id. Falls back to the id itself, which is what
   * an agent picked before the model left the account shows — better than
   * pretending it ran on something else.
   */
  modelName(id: string | null | undefined): string {
    if (!id) return "Default";
    const found = this.models.find((m) => m.id === id);
    return found ? modelLabel(found.display_name) : id;
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
      this.pruneQueues();
      this.applyInitialSelection();
    } catch (e) {
      this.error = String(e);
    }
  }

  /**
   * Re-ask git which merged agents are holding work. Swallows failures: the
   * previous answer is a better sidebar than an error banner over a reading.
   */
  private async loadHoldingWork() {
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
    this.clearDiff();
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
    this.clearDiff();
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
    if (tab === "diff" && !this.diffLoading) this.loadDiff();
  }

  private clearDiff() {
    this.diff = null;
    this.diffError = null;
    this.diffLoading = false;
  }

  /**
   * Load the selected agent's diff. Guards against a slow response landing
   * after the user has moved to a different agent.
   */
  async loadDiff() {
    const id = this.selectedAgentId;
    if (!id) return;
    this.diffLoading = true;
    this.diffError = null;
    try {
      const diff = await api.agentDiff(id);
      if (this.selectedAgentId !== id) return;
      this.diff = diff;
    } catch (e) {
      if (this.selectedAgentId !== id) return;
      this.diffError = String(e);
      this.diff = null;
    } finally {
      if (this.selectedAgentId === id) this.diffLoading = false;
    }
  }

  /** Commit everything in the selected agent's worktree, then refresh the diff. */
  async commit(message: string) {
    const id = this.selectedAgentId;
    if (!id) return false;
    this.committing = true;
    this.diffError = null;
    try {
      await api.agentCommit(id, message);
      // Committing is the other way a merged agent starts holding work the
      // project hasn't got — git status goes quiet but the tip moves.
      await this.loadHoldingWork();
      if (this.selectedAgentId === id) await this.loadDiff();
      return true;
    } catch (e) {
      if (this.selectedAgentId === id) this.diffError = String(e);
      return false;
    } finally {
      if (this.selectedAgentId === id) this.committing = false;
    }
  }

  /** The branches the selected agent's work could merge into. */
  async loadBranches() {
    const projectId = this.selectedAgent?.project_id;
    if (!projectId) {
      this.branches = null;
      return;
    }
    try {
      const branches = await api.projectBranches(projectId);
      if (this.selectedAgent?.project_id !== projectId) return;
      this.branches = branches;
    } catch {
      // Not worth an error banner: without a list the merge control just says
      // it has nothing to offer.
      if (this.selectedAgent?.project_id === projectId) this.branches = null;
    }
  }

  /**
   * Merge the selected agent's branch onto `target`. The agent survives — only
   * a reap destroys anything — so this refreshes rather than clears.
   */
  async merge(target: string) {
    const id = this.selectedAgentId;
    if (!id) return false;
    this.merging = true;
    this.diffError = null;
    try {
      const outcome = await api.agentMerge(id, target);
      if (outcome.outcome === "conflict") {
        this.conflicts[id] = { target: outcome.target, files: outcome.files };
        return false;
      }
      delete this.conflicts[id];
      // The agent now carries where it merged, and the branch list has moved on.
      await this.refresh();
      if (this.selectedAgentId === id) {
        await Promise.all([this.loadDiff(), this.loadBranches()]);
      }
      return true;
    } catch (e) {
      if (this.selectedAgentId === id) this.diffError = String(e);
      return false;
    } finally {
      if (this.selectedAgentId === id) this.merging = false;
    }
  }

  /** Put a conflict away without resolving it. The project is untouched either way. */
  dismissConflict(agentId: string) {
    delete this.conflicts[agentId];
  }

  /**
   * Hand the selected agent's conflicted merge to a resolver: a new agent, cut
   * from this one's branch, that merges the target in and settles the
   * conflicts. When its turn completes the backend finishes the merge, and
   * both agents move to Delivered. Opens the resolver, so the user watches the
   * work that's now happening rather than the agent that's waiting on it.
   */
  async resolveConflict() {
    const id = this.selectedAgentId;
    const conflict = id ? this.conflicts[id] : undefined;
    if (!id || !conflict || this.resolving) return false;
    this.resolving = true;
    this.error = null;
    try {
      const resolver = await api.resolveConflict(
        id,
        conflict.target,
        conflict.files,
        this.defaultSpawnModel || null,
        this.defaultSpawnEffort || null,
      );
      delete this.conflicts[id];
      this.agents.push(resolver);
      this.selectAgent(resolver.id);
      return true;
    } catch (e) {
      this.error = String(e);
      return false;
    } finally {
      this.resolving = false;
    }
  }

  /**
   * The resolver still working on this agent's conflict, if there is one —
   * the most recent that hasn't merged yet.
   */
  resolverFor(agentId: string): Agent | null {
    let found: Agent | null = null;
    for (const a of this.agents) {
      if (a.resolves?.agent_id !== agentId || a.merged_at) continue;
      if (!found || Date.parse(a.spawned_at) > Date.parse(found.spawned_at)) found = a;
    }
    return found;
  }

  /**
   * Bring an Agent on screen wherever it lives: switch to its Project, then
   * select it. False if there is no such Agent any more — its log can outlive
   * it, so a caller working from the logs can be holding a Reaped id.
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
    this.orphanBannerDismissed = true;
  }

  /**
   * Remember a model pick as the default for the next Spawn. Called for every
   * Turn the user starts, so "the model I ran last" is what the next one opens
   * on, whether that Turn was a Spawn or a reply to a running conversation.
   */
  rememberTurn(model: string, effort: string, mode: string) {
    this.preferredModel = model;
    this.preferredEffort = effort;
    this.preferredMode = mode;
    try {
      localStorage.setItem(MODEL_KEY, model);
      localStorage.setItem(EFFORT_KEY, effort);
      localStorage.setItem(MODE_KEY, mode);
    } catch {
      // A preference isn't worth failing a turn over.
    }
  }

  /**
   * Start a new agent on the selected project. `model` is the picker's value;
   * empty means no `--model` at all. Returns whether it started — the draft
   * page keeps the prompt on failure so the user can retry rather than retype.
   */
  async spawn(
    prompt: string,
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
        model || null,
        effort || null,
        mode || null,
      );
      this.rememberTurn(model, effort, mode);
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
   * Send a follow-up prompt to the selected agent, putting it back to work in
   * the worktree it already has, on `model` and in `mode`. Returns whether the
   * agent took it.
   */
  async resume(prompt: string, model: string, effort: string, mode: string) {
    const id = this.selectedAgentId;
    if (!id || !prompt.trim() || this.sending) return false;
    return this.sendTurn(id, prompt, model, effort, mode);
  }

  /**
   * Start a Turn on a named Agent. Takes an id rather than reading the selection
   * because a queue drains whether or not its Agent is the one on screen, and
   * `sending` — a flag the composer reads — only speaks for the Agent it shows.
   */
  private async sendTurn(
    id: string,
    prompt: string,
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
        model || null,
        effort || null,
        mode || null,
      );
      this.rememberTurn(model, effort, mode);
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

  /** The queue behind one Agent, oldest first. */
  queueFor(id: string | null): QueuedMessage[] {
    return id ? this.queues[id] ?? [] : [];
  }

  /**
   * Hold a prompt for the selected Agent until its current Turn ends. Returns
   * whether it was taken, on the same terms as a send: false leaves the text in
   * the composer rather than losing it.
   */
  enqueue(prompt: string, model: string, effort: string, mode: string): boolean {
    const id = this.selectedAgentId;
    const agent = this.selectedAgent;
    if (!id || !agent?.session_id || !prompt.trim()) return false;
    this.queues[id] = [
      ...this.queueFor(id),
      { id: queuedId(), prompt: prompt.trim(), model, effort, mode },
    ];
    this.saveQueues();
    // The Turn may have ended between the user typing and pressing Enter; in
    // that case the message shouldn't sit there waiting for a Turn that is
    // already over. drainQueue only fires on an Agent that is free.
    if (agent.state !== "running") this.drainQueue(id);
    return true;
  }

  /** Drop one queued message — the ✕ beside it in the strip. */
  unqueue(agentId: string, messageId: string) {
    const left = this.queueFor(agentId).filter((m) => m.id !== messageId);
    if (left.length === 0) delete this.queues[agentId];
    else this.queues[agentId] = left;
    this.saveQueues();
  }

  /** Drop everything waiting behind an Agent. */
  clearQueue(agentId: string) {
    if (!this.queues[agentId]) return;
    delete this.queues[agentId];
    this.saveQueues();
  }

  /**
   * Send the next queued message to an Agent that is free, and take it off the
   * queue once it's away. Left on the queue if the send fails, so a rejected
   * follow-up stays visible beside the error rather than vanishing.
   *
   * Only called for an Agent that ended a Turn *cleanly*, or when the user asks
   * for it by hand. A Stop or a Fail wants a human look — firing the rest of the
   * queue into a Stopped Agent would undo the interrupt the user just made.
   */
  async drainQueue(agentId: string) {
    const [next] = this.queueFor(agentId);
    if (!next) return;
    const agent = this.agents.find((a) => a.id === agentId);
    if (!agent || agent.state === "running" || !agent.session_id) return;
    const sent = await this.sendTurn(
      agentId,
      next.prompt,
      next.model,
      next.effort,
      // Queued before modes existed: run it as every Turn ran back then.
      next.mode ?? DEFAULT_MODE,
    );
    if (sent) {
      this.unqueue(agentId, next.id);
    }
  }

  private saveQueues() {
    try {
      localStorage.setItem(QUEUE_KEY, JSON.stringify(this.queues));
    } catch {
      // The queue still works in memory; persistence isn't worth an error over.
    }
  }

  /**
   * Forget queues belonging to Agents that no longer exist — Reaped in another
   * window, or gone since the last launch. A stored queue outliving its Agent
   * would otherwise never be sent and never be seen.
   */
  private pruneQueues() {
    let dropped = false;
    for (const id of Object.keys(this.queues)) {
      if (!this.agents.some((a) => a.id === id)) {
        delete this.queues[id];
        dropped = true;
      }
    }
    if (dropped) this.saveQueues();
  }

  async stopAgent(id: string) {
    try {
      await api.stopAgent(id);
    } catch (e) {
      this.error = String(e);
    }
  }

  async reapAgent(id: string) {
    try {
      await api.reapAgent(id);
      this.agents = this.agents.filter((a) => a.id !== id);
      delete this.eventsByAgent[id];
      this.hydrated.delete(id);
      this.clearQueue(id);
      if (this.selectedAgentId === id) {
        this.selectedAgentId = null;
        this.clearDiff();
      }
    } catch (e) {
      this.error = String(e);
    }
  }
}

export const store = new AppStore();
