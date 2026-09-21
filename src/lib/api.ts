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
  /** The effort its turns run on, as `claude --effort` takes it. Null = Claude Code's default. */
  effort?: string | null;
  /**
   * What its turns may do without asking, as `claude --permission-mode` takes
   * it. Null = YOLO, the mode every agent ran in before the picker existed.
   */
  permission_mode?: string | null;
  /** Turns started so far, including the opening one. */
  turns: number;
  /**
   * A short name Claude wrote for this agent's work. Rewritten at the end of
   * each of the first three turns, then frozen. Null until the opening turn
   * ends, and on agents recorded before titles existed — fall back to the
   * prompt via `store.agentName`.
   */
  title?: string | null;
  spawned_at: string;
  /** When the current (or last) turn began. Absent on agents recorded before turn timing. */
  turn_started_at?: string | null;
  exited_at?: string | null;
  exit_code?: number | null;
  fail_reason?: string | null;
  /**
   * The branch this agent's work was last merged into, and when. Absent until
   * a merge succeeds. A record of what happened, not a state — a merged agent
   * can still be resumed, committed, and merged again.
   */
  merged_branch?: string | null;
  merged_at?: string | null;
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

/**
 * Anthropic's display names lead with "Claude", which is noise in an app that
 * runs nothing else: "Claude Opus 4.5" is just "Opus 4.5" here.
 */
export function modelLabel(displayName: string): string {
  return displayName.replace(/^claude\s+/i, "");
}

/**
 * How hard Claude Code works a turn, as `claude --effort` takes it. The picker
 * offers these under each model; the empty value passes no `--effort` and
 * leaves the level to Claude Code.
 */
export const EFFORTS = ["low", "medium", "high", "xhigh", "max"] as const;

/** The picker's value for "no effort named". */
export const DEFAULT_EFFORT = "";

/**
 * What a turn may do without asking, as `claude --permission-mode` takes it.
 * Only the two modes that never stop to ask are offered: the app hands `claude`
 * no input channel, so a mode that puts up a permission prompt would hang the
 * turn with nowhere to answer it. Isolation is the safety story here — the
 * worktree is the sandbox — so acting freely inside one is the default, and
 * plan is for the agent you want thinking before it touches anything. The
 * names here are the UI's; the ids are what `claude` takes. The tone is the
 * colour the picker wears once a mode is chosen — every mode gets one, so the
 * control looks the same whichever is picked and only the colour says which.
 */
export const MODES = [
  { id: "bypassPermissions", name: "YOLO", note: "acts freely in its worktree", tone: "warn" },
  { id: "plan", name: "Plan", note: "reads and proposes, writes nothing", tone: "accent" },
] as const;

/** The mode a turn runs in unless the user picks otherwise. */
export const DEFAULT_MODE = "bypassPermissions";

/** What to call a mode. Falls back to the id, for a mode this build predates. */
export function modeLabel(id: string | null | undefined): string {
  if (!id) return modeLabel(DEFAULT_MODE);
  return MODES.find((m) => m.id === id)?.name ?? id;
}

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

/** The branches a merge can target, as the picker offers them. */
export type Branches = {
  /** The project's checked-out branch, or null on a detached HEAD. */
  current: string | null;
  /** Mergeable branches: current first, then alphabetical, agent branches omitted. */
  names: string[];
};

export type Merged = {
  target: string;
  /** The merge commit now at the tip of `target`. */
  sha: string;
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
  /**
   * Project branches that already contain these commits, so merging into them
   * would do nothing. Empty until the work is merged, and empty again once the
   * agent commits something new.
   */
  merged_into: string[];
};

/** What one Model cost across the Turns that used it. */
export type ModelUsage = {
  /** The model as `--model` names it. */
  model: string;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_creation_tokens: number;
  /** Claude Code's own cost figure for those tokens, in USD. */
  cost_usd: number;
  context_window: number | null;
};

/** One Agent's share of the total, biggest spender first. */
export type AgentUsage = {
  agent_id: string;
  models: ModelUsage[];
  /** Turns that reached an answer. */
  turns: number;
  last_at: string | null;
};

/** One rate-limit window on the account. */
export type LimitWindow = {
  /** Claude Code's name for it: `five_hour`, `seven_day`, … */
  kind: string;
  /** How much of the window is spent, 0–1. */
  utilization: number;
  /** Unix seconds at which it rolls over. */
  resets_at: number;
};

export type Limits = {
  windows: LimitWindow[];
  /** When an Agent was last told this — a snapshot, not a live reading. */
  observed_at: string;
  using_overage: boolean;
  status: string | null;
};

export type UsageSummary = {
  agents: AgentUsage[];
  /** Null until some Agent has been told the account's limits. */
  limits: Limits | null;
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
  spawnAgent: (
    projectId: string,
    prompt: string,
    model: string | null,
    effort: string | null,
    permissionMode: string | null,
  ) =>
    invoke<Agent>("spawn_agent", {
      projectId,
      prompt,
      model,
      effort,
      permissionMode,
    }),
  resumeAgent: (
    agentId: string,
    prompt: string,
    model: string | null,
    effort: string | null,
    permissionMode: string | null,
  ) =>
    invoke<Agent>("resume_agent", {
      agentId,
      prompt,
      model,
      effort,
      permissionMode,
    }),
  stopAgent: (agentId: string) => invoke<void>("stop_agent", { agentId }),
  reapAgent: (agentId: string) => invoke<void>("reap_agent", { agentId }),
  agentEvents: (agentId: string) =>
    invoke<AgentEvent[]>("agent_events", { agentId }),
  agentDiff: (agentId: string) => invoke<WorktreeDiff>("agent_diff", { agentId }),
  agentCommit: (agentId: string, message: string) =>
    invoke<Commit>("agent_commit", { agentId, message }),
  agentMerge: (agentId: string, target: string) =>
    invoke<Merged>("agent_merge", { agentId, target }),
  projectBranches: (projectId: string) =>
    invoke<Branches>("project_branches", { projectId }),

  listModels: () => invoke<ModelInfo[]>("list_models"),

  usageSummary: () => invoke<UsageSummary>("usage_summary"),

  startupOrphans: () => invoke<Agent[]>("startup_orphans"),
};

export const events = {
  onAgentEvent: (fn: (payload: AgentEventPayload) => void): Promise<UnlistenFn> =>
    listen<AgentEventPayload>("agent-event", (msg) => fn(msg.payload)),

  onAgentStateChanged: (fn: (agent: Agent) => void): Promise<UnlistenFn> =>
    listen<Agent>("agent-state-changed", (msg) => fn(msg.payload)),
};
