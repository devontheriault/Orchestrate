import {
  api,
  DEFAULT_MODEL,
  events,
  type Agent,
  type AgentEvent,
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
  /** The most recently spawned running agent, for a "what's it doing" line. */
  latestRunning: Agent | null;
};

const NO_ACTIVITY: ProjectActivity = {
  running: 0,
  attention: 0,
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
 * The model the user picked last, for any Turn — a Spawn or a Resume — so the
 * spawn modal opens on their habitual choice instead of resetting every time.
 * localStorage, not sessionStorage: a preference should outlive the window.
 *
 * The stored value is the picker's own, so the empty string is a real answer —
 * the user asking for Claude Code's default. Only a missing key means they have
 * never picked, which is the one case the fallback below gets to speak.
 */
const MODEL_KEY = "cw:preferred-model";

function readPreferredModel(): string | null {
  try {
    return localStorage.getItem(MODEL_KEY);
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

  /** Which body the detail pane shows for the selected agent. */
  detailTab = $state<"output" | "diff">("output");

  /** Diff of the selected agent's worktree. Cleared when the selection moves. */
  diff = $state<WorktreeDiff | null>(null);
  diffLoading = $state<boolean>(false);
  diffError = $state<string | null>(null);
  committing = $state<boolean>(false);

  /** A follow-up prompt is in flight for the selected agent. */
  sending = $state<boolean>(false);

  /** The model the user picked for the most recent Turn. Null = never picked. */
  preferredModel = $state<string | null>(readPreferredModel());

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

  private unlisteners: UnlistenFn[] = [];
  private started = false;

  agentsForSelectedProject = $derived(
    this.selectedProjectId
      ? this.agents.filter((a) => a.project_id === this.selectedProjectId)
      : [],
  );

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
   * The model the most recently spawned Agent ran on, as a picker value. Stands
   * in for a stored preference on a profile that has none yet — the Agents on
   * disk are a record of the user's picks too, and reading them means the modal
   * is right on the first spawn after an update rather than the second.
   */
  private lastSpawnedModel = $derived.by(() => {
    let latest: Agent | null = null;
    for (const a of this.agents) {
      if (!latest || Date.parse(a.spawned_at) >= Date.parse(latest.spawned_at)) {
        latest = a;
      }
    }
    return latest ? latest.model ?? DEFAULT_MODEL : null;
  });

  /** What the spawn modal's picker opens on: the last model the user ran on. */
  defaultSpawnModel = $derived(
    this.preferredModel ?? this.lastSpawnedModel ?? DEFAULT_MODEL,
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
   * Anthropic's name for a model id. Falls back to the id itself, which is what
   * an agent picked before the model left the account shows — better than
   * pretending it ran on something else.
   */
  modelName(id: string | null | undefined): string {
    if (!id) return "Default";
    return this.models.find((m) => m.id === id)?.display_name ?? id;
  }

  async stop() {
    for (const un of this.unlisteners) un();
    this.unlisteners = [];
    this.started = false;
  }

  async refresh() {
    try {
      this.error = null;
      [this.projects, this.agents, this.orphans] = await Promise.all([
        api.listProjects(),
        api.listAgents(),
        api.startupOrphans(),
      ]);
      this.applyInitialSelection();
    } catch (e) {
      this.error = String(e);
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

  async addProject(name: string, path: string) {
    try {
      const p = await api.addProject(name, path);
      this.projects.push(p);
      this.selectedProjectId = p.id;
    } catch (e) {
      this.error = String(e);
    }
  }

  async removeProject(id: string) {
    try {
      await api.removeProject(id);
      this.projects = this.projects.filter((p) => p.id !== id);
      if (this.selectedProjectId === id) {
        this.selectedProjectId = this.projects[0]?.id ?? null;
      }
    } catch (e) {
      this.error = String(e);
    }
  }

  selectProject(id: string) {
    this.selectedProjectId = id;
    this.selectAgent(null);
  }

  selectAgent(id: string | null) {
    if (id === this.selectedAgentId) return;
    this.selectedAgentId = id;
    this.clearDiff();
    if (id) this.hydrateEvents(id);
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
      if (this.selectedAgentId === id) await this.loadDiff();
      return true;
    } catch (e) {
      if (this.selectedAgentId === id) this.diffError = String(e);
      return false;
    } finally {
      if (this.selectedAgentId === id) this.committing = false;
    }
  }

  jumpToFirstOrphan() {
    if (this.orphans.length === 0) return;
    const first = this.orphans[0];
    if (this.projects.some((p) => p.id === first.project_id)) {
      this.selectedProjectId = first.project_id;
    }
    this.selectAgent(first.id);
    this.orphanBannerDismissed = true;
  }

  /**
   * Remember a model pick as the default for the next Spawn. Called for every
   * Turn the user starts, so "the model I ran last" is what the next one opens
   * on, whether that Turn was a Spawn or a reply to a running conversation.
   */
  rememberModel(model: string) {
    this.preferredModel = model;
    try {
      localStorage.setItem(MODEL_KEY, model);
    } catch {
      // A preference isn't worth failing a turn over.
    }
  }

  /** `model` is the picker's value; empty means no `--model` at all. */
  async spawn(prompt: string, model: string) {
    if (!this.selectedProjectId) return;
    try {
      const agent = await api.spawnAgent(
        this.selectedProjectId,
        prompt,
        model || null,
      );
      this.rememberModel(model);
      this.agents.push(agent);
      this.selectAgent(agent.id);
    } catch (e) {
      this.error = String(e);
    }
  }

  /**
   * Send a follow-up prompt to the selected agent, putting it back to work in
   * the worktree it already has, on `model`. Returns whether the agent took it.
   */
  async resume(prompt: string, model: string) {
    const id = this.selectedAgentId;
    if (!id || !prompt.trim() || this.sending) return false;
    this.sending = true;
    this.error = null;
    try {
      const agent = await api.resumeAgent(id, prompt, model || null);
      this.rememberModel(model);
      const i = this.agents.findIndex((a) => a.id === agent.id);
      if (i >= 0) this.agents[i] = agent;
      // It's working again, so it's nobody's leftover any more.
      this.orphans = this.orphans.filter((o) => o.id !== id);
      return true;
    } catch (e) {
      this.error = String(e);
      return false;
    } finally {
      this.sending = false;
    }
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
