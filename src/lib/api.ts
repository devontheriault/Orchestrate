/**
 * The IPC boundary: every call the backend answers, the events it pushes, and
 * the shapes that cross over. Types here mirror the Rust structs they
 * deserialize from (`src-tauri/src/domain.rs`, `git/`, `usage/`), so a field
 * added there is added here.
 *
 * Nearly every call is for the Host, the process that owns the Agents, and
 * goes through `host()`; the window passes it on untouched. The few left on
 * `invoke` are the window's own: the clipboard and files dropped on it.
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
  /** The rest of how its turns run, set for the agent as a whole. Absent on older agents. */
  options?: AgentOptions;
  /** Turns started so far, including the opening one. */
  turns: number;
  /**
   * A short name Claude wrote for this agent's work. Rewritten at the end of
   * each of the first three turns, then frozen. Null until the opening turn
   * ends, and on agents recorded before titles existed — fall back to the
   * prompt via `store.agentName`.
   */
  title?: string | null;
  /** The Title the user gave it with `/rename`. Wins over `title`. */
  user_title?: string | null;
  /** The colour the user tagged it with, from `/color`: one of `TAGS`. */
  color?: string | null;
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
   * Why the last merge didn't reach the project's remote, while it hasn't. The
   * merge itself stands. Null once pushed, or when there's no remote.
   */
  push_error?: string | null;
  /**
   * Set on a resolver: the agent whose merge conflicted, and the branch it was
   * headed for. When the resolver's turn completes cleanly the app finishes
   * that merge and records it on both.
   */
  resolves?: Resolution | null;
  /**
   * What the user has said that the agent wasn't free to hear yet, oldest
   * first. Kept by the Host, which sends the next one when a Turn Completes.
   */
  queue: QueuedMessage[];
};

/**
 * A prompt waiting for its agent to be free, with the picks it was queued
 * under. Null leaves a pick to Claude Code, as it does on a Turn.
 */
export type QueuedMessage = {
  id: string;
  prompt: string;
  /** Files attached to it, by path. Absent when there are none. */
  attachments?: string[];
  model: string | null;
  effort: string | null;
  permission_mode: string | null;
};

/**
 * Claude Code settings an agent's every turn runs with, set once rather than
 * picked per prompt. Null leaves each to Claude Code's own configuration.
 */
export type AgentOptions = {
  /** The model Claude consults at key moments: "fable", "opus", "sonnet" or "off". */
  advisor?: string | null;
  /** The output style, by the name `claude` lists it under, e.g. "Concise". */
  output_style?: string | null;
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

/**
 * One slash command the composer's `/` menu offers, as `claude` itself lists
 * it for a directory: its built-ins, the user's skills and plugins, and the
 * Project's own commands.
 */
export type SlashCommand = {
  /** What follows the `/`, e.g. "compact" or "mattpocock-skills:tdd". */
  name: string;
  description: string;
  /** What it takes after its name, e.g. "[interval] [prompt]"; empty if unsaid. */
  argument_hint: string;
  /** Other names `claude` accepts for it, e.g. "tdd". */
  aliases: string[];
};

/** What `claude` offers a turn started in one directory. */
export type Offered = {
  commands: SlashCommand[];
  /** Output styles by name, "default" first. */
  output_styles: string[];
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
  /** There is work not yet in a commit — a discard now would destroy it. */
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

/** Every Claude Code session on this computer, the agents' among them. */
export type AccountUsage = {
  /** Priced at list rates — transcripts don't carry Claude Code's own figure. */
  models: ModelUsage[];
  slots: SlotUsage[];
  /** Replies that ended a turn. */
  turns: number;
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
  account: AccountUsage;
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

/** Ask the Host something. The call names and arguments are `host/calls.rs`'s. */
const host = <T>(method: string, args: Record<string, unknown> = {}) =>
  invoke<T>("host", { method, args });

/**
 * Where the window stands with its Host. `updating`: the Host is an older
 * build, finishing its running Turns before the new one takes over.
 * `outdated`: the Host is newer than this window.
 */
export type HostStatus =
  | { state: "connecting"; error: string | null }
  | { state: "connected"; instance: string; version: string }
  | { state: "updating"; version: string }
  | { state: "outdated"; version: string };

export const api = {
  hostStatus: () => invoke<HostStatus>("host_status"),

  listProjects: () => host<Project[]>("list_projects"),
  /** True when the folder isn't a Git repository with a commit yet. */
  projectNeedsSetup: (path: string) => host<boolean>("project_needs_setup", { path }),
  addProject: (name: string, path: string, setUp: boolean) =>
    host<Project>("add_project", { name, path, setUp }),
  removeProject: (id: string) => host<void>("remove_project", { id }),

  listAgents: () => host<Agent[]>("list_agents"),
  spawnAgent: (
    projectId: string,
    prompt: string,
    attachments: string[],
    model: string | null,
    effort: string | null,
    permissionMode: string | null,
    options: AgentOptions,
  ) =>
    host<Agent>("spawn_agent", {
      projectId,
      prompt,
      attachments,
      model,
      effort,
      permissionMode,
      options,
    }),
  /**
   * Say something to an agent. The Host starts a Turn if the agent is free and
   * queues it if not; the returned record says which.
   */
  sendMessage: (
    agentId: string,
    prompt: string,
    attachments: string[],
    model: string | null,
    effort: string | null,
    permissionMode: string | null,
  ) =>
    host<Agent>("send_message", {
      agentId,
      prompt,
      attachments,
      model,
      effort,
      permissionMode,
    }),
  /** Send the head of a queue that a Stop or a Fail held. */
  sendNext: (agentId: string) => host<Agent>("send_next", { agentId }),
  /** Add messages to the end of a queue without sending any. */
  queueMessages: (agentId: string, messages: QueuedMessage[]) =>
    host<Agent>("queue_messages", { agentId, messages }),
  removeQueued: (agentId: string, messageId: string) =>
    host<Agent>("remove_queued", { agentId, messageId }),
  clearQueue: (agentId: string) => host<Agent>("clear_queue", { agentId }),
  /**
   * Whether the Host keeps running while the user is logged out; null when it
   * isn't a service that could.
   */
  keepRunning: () => host<boolean | null>("keep_running"),
  setKeepRunning: (on: boolean) => host<void>("set_keep_running", { on }),
  /** Name an agent; null hands the naming back to Claude's title. */
  renameAgent: (agentId: string, name: string | null) =>
    host<Agent>("rename_agent", { agentId, name }),
  /** Tag an agent with one of `TAGS`, or untag it with null. */
  setAgentColor: (agentId: string, color: string | null) =>
    host<Agent>("set_agent_color", { agentId, color }),
  /** Set how an agent's turns run, from its next one on. */
  setAgentOptions: (agentId: string, options: AgentOptions) =>
    host<Agent>("set_agent_options", { agentId, options }),
  /**
   * Write a pasted file to disk so it can be attached by path, and return the
   * path. Sent as raw bytes rather than JSON; the name rides in a header,
   * which only carries ASCII.
   */
  saveAttachment: (name: string, bytes: Uint8Array) =>
    invoke<string>("save_attachment", bytes, {
      headers: { "x-name": name.replace(/[^\x20-\x7e]/g, "_") },
    }),
  /**
   * Save the image on the OS clipboard as `name` and return its path, or null
   * if there's none — for a paste whose event carried no files.
   */
  saveClipboardImage: (name: string) => invoke<string | null>("save_clipboard_image", { name }),
  /** An attached image's bytes, for its thumbnail. Refused for non-images. */
  attachmentPreview: (path: string) => invoke<ArrayBuffer>("attachment_preview", { path }),
  stopAgent: (agentId: string) => host<void>("stop_agent", { agentId }),
  discardAgent: (agentId: string) => host<void>("discard_agent", { agentId }),
  agentEvents: (agentId: string) =>
    host<AgentEvent[]>("agent_events", { agentId }),
  agentDiff: (agentId: string) => host<WorktreeDiff>("agent_diff", { agentId }),
  agentCommit: (agentId: string, message: string) =>
    host<Commit>("agent_commit", { agentId, message }),
  agentMerge: (agentId: string, target: string) =>
    host<MergeOutcome>("agent_merge", { agentId, target }),
  /** Push the last merge again, after its push failed. */
  pushMerge: (agentId: string) => host<Agent>("push_merge", { agentId }),
  /** Spawn a resolver for a merge of `agentId` into `target` that conflicted. */
  resolveConflict: (
    agentId: string,
    target: string,
    files: string[],
    model: string | null,
    effort: string | null,
  ) =>
    host<Agent>("resolve_conflict", { agentId, target, files, model, effort }),
  /** Ids of merged agents whose worktree still holds work the project lacks. */
  agentsHoldingWork: () => host<string[]>("agents_holding_work"),
  projectBranches: (projectId: string) =>
    host<Branches>("project_branches", { projectId }),

  listModels: () => host<ModelInfo[]>("list_models"),
  slashCommands: (dir: string) => host<Offered>("slash_commands", { dir }),

  usageSummary: () => host<UsageSummary>("usage_summary"),

  startupOrphans: () => host<Agent[]>("startup_orphans"),
  dismissOrphans: () => host<void>("dismiss_orphans"),
};

export const events = {
  onAgentEvent: (fn: (payload: AgentEventPayload) => void): Promise<UnlistenFn> =>
    listen<AgentEventPayload>("agent-event", (msg) => fn(msg.payload)),

  onAgentStateChanged: (fn: (agent: Agent) => void): Promise<UnlistenFn> =>
    listen<Agent>("agent-state-changed", (msg) => fn(msg.payload)),

  onHostStatus: (fn: (status: HostStatus) => void): Promise<UnlistenFn> =>
    listen<HostStatus>("host-status", (msg) => fn(msg.payload)),
};
