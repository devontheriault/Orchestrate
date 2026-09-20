import { api, events, type Agent, type AgentEvent, type Project } from "./api";
import type { UnlistenFn } from "@tauri-apps/api/event";

class AppStore {
  projects = $state<Project[]>([]);
  agents = $state<Agent[]>([]);
  orphans = $state<Agent[]>([]);
  eventsByAgent = $state<Record<string, AgentEvent[]>>({});

  selectedProjectId = $state<string | null>(null);
  selectedAgentId = $state<string | null>(null);

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
    this.selectedAgentId = null;
  }

  selectAgent(id: string | null) {
    this.selectedAgentId = id;
  }

  jumpToFirstOrphan() {
    if (this.orphans.length === 0) return;
    const first = this.orphans[0];
    if (this.projects.some((p) => p.id === first.project_id)) {
      this.selectedProjectId = first.project_id;
    }
    this.selectedAgentId = first.id;
    this.orphanBannerDismissed = true;
  }

  async spawn(prompt: string) {
    if (!this.selectedProjectId) return;
    try {
      const agent = await api.spawnAgent(this.selectedProjectId, prompt);
      this.agents.push(agent);
      this.selectedAgentId = agent.id;
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
      if (this.selectedAgentId === id) this.selectedAgentId = null;
    } catch (e) {
      this.error = String(e);
    }
  }
}

export const store = new AppStore();
