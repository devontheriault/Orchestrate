/**
 * The IPC boundary: every Tauri command the backend exposes, the two events it
 * pushes, and the shapes that cross over. Types here mirror the Rust structs
 * they deserialize from (`src-tauri/src/domain.rs`, `git/`, `usage.rs`), so a
 * field added there is added here.
 */
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
  /** Files handed over with the prompt, by absolute path. Absent when there are none. */
  attachments?: string[];
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
  /**
   * Set on a resolver: the agent whose merge conflicted, and the branch it was
   * headed for. When the resolver's turn completes cleanly the app finishes
   * that merge and records it on both.
   */
  resolves?: Resolution | null;
};

export type Resolution = {
  agent_id: string;
  target: string;
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

/**
 * How a merge came out. A conflict isn't an error: the project is untouched,
 * and the files that collided are what a resolver is handed.
 */
export type MergeOutcome =
  | ({ outcome: "merged" } & Merged)
  | { outcome: "conflict"; target: string; files: string[] };

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

/** What one Agent spent inside one quarter-hour slot, all Models together. */
export type SlotUsage = {
  /** Unix seconds at which the slot opens. */
  start: number;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_creation_tokens: number;
  cost_usd: number;
  /** Turns whose answer landed in the slot. */
  turns: number;
};

/** One Agent's share of the total, biggest spender first. */
export type AgentUsage = {
  agent_id: string;
  models: ModelUsage[];
  /** When the spend happened, oldest slot first; only slots with spend. */
  slots: SlotUsage[];
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
  /** True when the folder isn't a Git repository with a commit yet. */
  projectNeedsSetup: (path: string) => invoke<boolean>("project_needs_setup", { path }),
  addProject: (name: string, path: string, setUp: boolean) =>
    invoke<Project>("add_project", { name, path, setUp }),
  removeProject: (id: string) => invoke<void>("remove_project", { id }),

  listAgents: () => invoke<Agent[]>("list_agents"),
  spawnAgent: (
    projectId: string,
    prompt: string,
    attachments: string[],
    model: string | null,
    effort: string | null,
    permissionMode: string | null,
  ) =>
    invoke<Agent>("spawn_agent", {
      projectId,
      prompt,
      attachments,
      model,
      effort,
      permissionMode,
    }),
  resumeAgent: (
    agentId: string,
    prompt: string,
    attachments: string[],
    model: string | null,
    effort: string | null,
    permissionMode: string | null,
  ) =>
    invoke<Agent>("resume_agent", {
      agentId,
      prompt,
      attachments,
      model,
      effort,
      permissionMode,
    }),
  /**
   * Write a pasted file to disk so it can be attached by path, and return the
   * path. Sent as raw bytes rather than JSON; the name rides in a header,
   * which only carries ASCII.
   */
  saveAttachment: (name: string, bytes: Uint8Array) =>
    invoke<string>("save_attachment", bytes, {
      headers: { "x-name": name.replace(/[^\x20-\x7e]/g, "_") },
    }),
  /** An attached image's bytes, for its thumbnail. Refused for non-images. */
  attachmentPreview: (path: string) => invoke<ArrayBuffer>("attachment_preview", { path }),
  stopAgent: (agentId: string) => invoke<void>("stop_agent", { agentId }),
  reapAgent: (agentId: string) => invoke<void>("reap_agent", { agentId }),
  agentEvents: (agentId: string) =>
    invoke<AgentEvent[]>("agent_events", { agentId }),
  agentDiff: (agentId: string) => invoke<WorktreeDiff>("agent_diff", { agentId }),
  agentCommit: (agentId: string, message: string) =>
    invoke<Commit>("agent_commit", { agentId, message }),
  agentMerge: (agentId: string, target: string) =>
    invoke<MergeOutcome>("agent_merge", { agentId, target }),
  /** Spawn a resolver for a merge of `agentId` into `target` that conflicted. */
  resolveConflict: (
    agentId: string,
    target: string,
    files: string[],
    model: string | null,
    effort: string | null,
  ) =>
    invoke<Agent>("resolve_conflict", { agentId, target, files, model, effort }),
  /** Ids of merged agents whose worktree still holds work the project lacks. */
  agentsHoldingWork: () => invoke<string[]>("agents_holding_work"),
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
