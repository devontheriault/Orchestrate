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
  base_commit?: string | null;
  /** The Claude Code session to resume. Absent means this agent can't continue. */
  session_id?: string | null;
  /** The model its turns run on, as `claude --model` takes it. Null = Claude Code's default. */
  model?: string | null;
  /** Turns started so far, including the opening one. */
  turns: number;
  spawned_at: string;
  exited_at?: string | null;
  exit_code?: number | null;
  fail_reason?: string | null;
};

/**
 * One model this user's account can run, as Anthropic's Models API reports it.
 * The picker is filled from that call rather than a list baked into the app, so
 * it shows the user's real models — versions included — and picks up new ones
 * without a release here.
 */
export type ModelInfo = {
  /** The full model name, e.g. "claude-opus-4-5-20251101". What `--model` takes. */
  id: string;
  /** Anthropic's own name, e.g. "Claude Opus 4.5". */
  display_name: string;
};

/**
 * The picker's value for "no model named", which passes no `--model` and lets
 * Claude Code use whatever it is configured for. Always available, so the
 * picker still works when the models call fails.
 */
export const DEFAULT_MODEL = "";

export type ChangedFile = {
  path: string;
  /** Git status vs the base: A, M, D, T, … */
  status: string;
  /** null for binary files. */
  insertions: number | null;
  deletions: number | null;
};

export type Commit = {
  sha: string;
  subject: string;
};

export type WorktreeDiff = {
  base: string;
  files: ChangedFile[];
  /** Commits on the agent's branch, oldest first. */
  commits: Commit[];
  patch: string;
  truncated: boolean;
  /** There is work not yet in a commit — a reap now would destroy it. */
  uncommitted: boolean;
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
  spawnAgent: (projectId: string, prompt: string, model: string | null) =>
    invoke<Agent>("spawn_agent", { projectId, prompt, model }),
  resumeAgent: (agentId: string, prompt: string, model: string | null) =>
    invoke<Agent>("resume_agent", { agentId, prompt, model }),
  stopAgent: (agentId: string) => invoke<void>("stop_agent", { agentId }),
  reapAgent: (agentId: string) => invoke<void>("reap_agent", { agentId }),
  agentEvents: (agentId: string) =>
    invoke<AgentEvent[]>("agent_events", { agentId }),
  agentDiff: (agentId: string) => invoke<WorktreeDiff>("agent_diff", { agentId }),
  agentCommit: (agentId: string, message: string) =>
    invoke<Commit>("agent_commit", { agentId, message }),

  listModels: () => invoke<ModelInfo[]>("list_models"),

  startupOrphans: () => invoke<Agent[]>("startup_orphans"),
};

export const events = {
  onAgentEvent: (fn: (payload: AgentEventPayload) => void): Promise<UnlistenFn> =>
    listen<AgentEventPayload>("agent-event", (msg) => fn(msg.payload)),

  onAgentStateChanged: (fn: (agent: Agent) => void): Promise<UnlistenFn> =>
    listen<Agent>("agent-state-changed", (msg) => fn(msg.payload)),
};
