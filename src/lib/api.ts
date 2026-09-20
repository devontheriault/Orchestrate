import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type AgentState =
  | "running"
  | "completed"
  | "failed"
  | "stopped"
  | "orphaned";

export type Project = {
  id: string;
  name: string;
  path: string;
  added_at: string;
};

export type Task = {
  prompt: string;
};

export type Agent = {
  id: string;
  project_id: string;
  task: Task;
  state: AgentState;
  worktree_path: string;
  branch: string;
  spawned_at: string;
  exited_at?: string | null;
  exit_code?: number | null;
  fail_reason?: string | null;
};

export type AgentEvent = {
  ts: string;
  event: unknown;
};

export type AgentEventPayload = {
  agent_id: string;
  event: AgentEvent;
};

export const api = {
  listProjects: () => invoke<Project[]>("list_projects"),
  addProject: (name: string, path: string) =>
    invoke<Project>("add_project", { name, path }),
  removeProject: (id: string) => invoke<void>("remove_project", { id }),

  listAgents: () => invoke<Agent[]>("list_agents"),
  spawnAgent: (projectId: string, prompt: string) =>
    invoke<Agent>("spawn_agent", { projectId, prompt }),
  stopAgent: (agentId: string) => invoke<void>("stop_agent", { agentId }),
  reapAgent: (agentId: string) => invoke<void>("reap_agent", { agentId }),

  startupOrphans: () => invoke<Agent[]>("startup_orphans"),
};

export const events = {
  onAgentEvent: (fn: (payload: AgentEventPayload) => void): Promise<UnlistenFn> =>
    listen<AgentEventPayload>("agent-event", (msg) => fn(msg.payload)),

  onAgentStateChanged: (fn: (agent: Agent) => void): Promise<UnlistenFn> =>
    listen<Agent>("agent-state-changed", (msg) => fn(msg.payload)),
};
