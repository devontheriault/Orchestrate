import {
  api,
  events,
  type Agent,
  type AgentEvent,
  type Project,
  type WorktreeDiff,
} from "./api";
import type { UnlistenFn } from "@tauri-apps/api/event";

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

  orphanBannerDismissed = $state<boolean>(false);
  error = $state<string | null>(null);

  private unlisteners: UnlistenFn[] = [];
  private started = false;

  agentsForSelectedProject = $derived(
    this.selectedProjectId
      ? this.agents.filter((a) => a.project_id === this.selectedProjectId)
      : [],
  );

  selectedAgent = $derived(
    this.selectedAgentId
      ? this.agents.find((a) => a.id === this.selectedAgentId) ?? null
      : null,
  );

  eventsForSelected = $derived(
    this.selectedAgentId ? this.eventsByAgent[this.selectedAgentId] ?? [] : [],
  );

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

    await this.refresh();
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
      if (!this.selectedProjectId && this.projects.length > 0) {
        this.selectedProjectId = this.projects[0].id;
      }
    } catch (e) {
      this.error = String(e);
    }
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

  async spawn(prompt: string) {
    if (!this.selectedProjectId) return;
    try {
      const agent = await api.spawnAgent(this.selectedProjectId, prompt);
      this.agents.push(agent);
      this.selectAgent(agent.id);
    } catch (e) {
      this.error = String(e);
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
