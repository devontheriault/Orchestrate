/**
 * The IPC boundary: every call the backend answers, the events it pushes, and
 * the shapes that cross over. Types here mirror the Rust structs they
 * deserialize from (`src-tauri/src/domain.rs`, `git/`, `usage/`), so a field
 * added there is added here.
 *
 * Nearly every call is for a Host, the process that owns a machine's Agents,
 * and goes through `host()`; the window passes it on untouched. There is one
 * Host per machine the window knows (see `state/hosts.svelte.ts`): `local`,
 * this machine's, and any added by Tailscale name. A call about an Agent goes
 * to the Host the Agent lives on, looked up in `agentHosts`; everything else
 * names its Host. The few calls left on `invoke` are the window's own: the
 * clipboard, files dropped on it, and the list of Hosts.
 */
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type AgentState =
  | "running"
  | "completed"
  | "failed"
  | "stopped"
  | "orphaned";

/** A checkout on one Host, as that Host lists it. */
export type Project = {
  id: string;
  name: string;
  path: string;
  added_at: string;
  /** Cloned by the Host itself for an agent spawned onto it from elsewhere. */
  cloned: boolean;
  /** Where its remote is, as git has it; null for a repository with none. */
  remote: string | null;
  /** The Host it's on. Stamped by the window as it lists them. */
  host: string;
};

export type Task = {
  prompt: string;
  /** Files handed over with the prompt, by absolute path. Absent when there are none. */
  attachments?: string[];
};

export type Agent = {
  id: string;
  /**
   * The project it belongs to. As the Host sends it, that Host's own id for
   * its checkout; the store swaps in the id of the project as the window
   * groups them — one project across every Host with a checkout of it — and
   * keeps the Host's in `home_project_id`.
   */
  project_id: string;
  home_project_id?: string;
  /** The Host it lives on. */
  host: string;
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
   * The last merge went into a branch that tracks a remote and isn't pushed
   * there yet: the user didn't ask, or the push failed. The merge itself
   * stands; the push is offered.
   */
  unpushed?: boolean;
  /** Why the last push of the merge failed, while it hasn't gone. */
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
 * An agent's committed work, put on the project's remote by its Host for an
 * agent on another Host to pick up (ADR 0013). Mirrors `handoff::Handoff`.
 */
export type Handoff = {
  /** The branch on the remote the work is waiting on. */
  branch: string;
  /** The agent's Base, which the new agent's diff is read against too. */
  base_commit: string | null;
  /** What the new agent is told about the work, ahead of the user's prompt. */
  brief: string;
  /** The agent's Title, which the new one goes by until Claude names it. */
  title: string | null;
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
  /** Whether the merge was to push `target` once it landed. */
  push?: boolean;
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

/** A Claude Code plugin installed on a Host. */
export type InstalledPlugin = {
  /** `name@marketplace`. */
  id: string;
  version: string;
  /** "user", "project" or "local": where it was installed, and so where it's changed. */
  scope: string;
  enabled: boolean;
  /** From the plugin's manifest; empty if it gives none. */
  description: string;
};

/** A plugin a marketplace offers that isn't installed yet. */
export type AvailablePlugin = {
  /** `name@marketplace`, what an install names. */
  id: string;
  name: string;
  description: string;
  marketplace: string;
  /** Installs so far, where the marketplace counts them. */
  installs: number | null;
};

export type Marketplace = {
  name: string;
  /** "github", "git", "url", "directory"… */
  source: string;
  /** Where it comes from: `owner/repo`, a URL or a path. */
  location: string;
};

/** What the `/plugin` window shows, as `claude plugin` sees it from one directory. */
export type PluginCatalog = {
  installed: InstalledPlugin[];
  /** Most installed first. */
  available: AvailablePlugin[];
  marketplaces: Marketplace[];
};

/**
 * A change to the plugins. `acceptCommand` is the hash of a command a
 * marketplace declared, which the user has seen and agreed to run.
 */
export type PluginChange =
  | { do: "install"; plugin: string; acceptCommand?: string }
  | { do: "uninstall" | "enable" | "disable"; plugin: string; scope: string }
  | { do: "update"; plugin: string; scope: string; acceptCommand?: string }
  | { do: "add_marketplace"; source: string }
  | { do: "remove_marketplace"; name: string }
  /** Every marketplace, with no name. */
  | { do: "update_marketplace"; name?: string };

/**
 * How a change went, when it didn't fail: done, or waiting on the user to
 * agree to a command the plugin's marketplace wants to run.
 */
export type PluginOutcome =
  | { outcome: "done"; message: string }
  | { outcome: "confirm"; message: string; command: string; sha256: string };

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
  /** Those of `names` that track a remote, so a merge into them can push. */
  pushable: string[];
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

/** The id of this machine's Host. */
export const LOCAL = "local";

/** Ask a Host something. The call names and arguments are `host/calls.rs`'s. */
const host = <T>(hostId: string, method: string, args: Record<string, unknown> = {}) =>
  invoke<T>("host", { host: hostId, method, args });

/**
 * Which Host each agent lives on, so a call about one goes there. Filled as
 * agents arrive (`route`); an agent this window hasn't seen yet is looked
 * for on this machine's.
 */
const agentHosts = new Map<string, string>();
const on = (agentId: string) => agentHosts.get(agentId) ?? LOCAL;

/** Records from `hostId`, stamped with it. */
const stamp = <T extends object>(hostId: string, items: T[]): (T & { host: string })[] =>
  items.map((item) => ({ ...item, host: hostId }));

/**
 * Where the window stands with one Host. `updating`: this machine's Host is
 * an older build, finishing its running Turns before this one takes over.
 * `outdated`: the Host is newer than this window. `behind`: another machine's
 * Host is too old to talk to until it's updated there. `refused`: another
 * machine's Host won't serve this window — it isn't the same Tailscale user.
 */
export type HostStatus =
  | { state: "connecting"; error: string | null }
  | { state: "connected"; instance: string; version: string; name: string }
  | { state: "updating"; version: string }
  | { state: "outdated"; version: string }
  | { state: "behind"; version: string }
  | { state: "refused"; reason: string };

export type HostInfo = {
  /** `local`, or the Tailscale name the Host was added by. */
  id: string;
  local: boolean;
  status: HostStatus;
};

/** One of the user's other machines on the tailnet, to add as a Host. */
export type Machine = {
  /** Its MagicDNS name, the one to add it by. */
  name: string;
  os: string;
  online: boolean;
};

export const api = {
  /** Every Host this window knows, and where it stands with each. */
  hosts: () => invoke<HostInfo[]>("hosts"),
  addHost: (name: string) => invoke<HostInfo>("add_host", { name }),
  /** The user's other machines, as this machine's Tailscale sees them. */
  tailnetMachines: () => invoke<Machine[]>("tailnet_machines"),
  removeHost: (id: string) => invoke<void>("remove_host", { id }),

  /** Note which Host an agent lives on, so calls about it go there. */
  route(agent: Pick<Agent, "id" | "host">) {
    agentHosts.set(agent.id, agent.host);
  },

  listProjects: async (hostId: string) =>
    stamp(hostId, await host<Omit<Project, "host">[]>(hostId, "list_projects")),
  /** True when the folder isn't a Git repository with a commit yet. */
  projectNeedsSetup: (path: string) => host<boolean>(LOCAL, "project_needs_setup", { path }),
  /** Register a folder on this machine — the one the folder picker browses. */
  addProject: async (name: string, path: string, setUp: boolean) => ({
    ...(await host<Omit<Project, "host">>(LOCAL, "add_project", { name, path, setUp })),
    host: LOCAL,
  }),
  removeProject: (hostId: string, id: string) => host<void>(hostId, "remove_project", { id }),
  /** Have a Host clone a project it has no checkout of, from its remote. */
  cloneProject: async (hostId: string, name: string, url: string) => ({
    ...(await host<Omit<Project, "host">>(hostId, "clone_project", { name, url })),
    host: hostId,
  }),

  listAgents: async (hostId: string) =>
    stamp(hostId, await host<Omit<Agent, "host">[]>(hostId, "list_agents")),
  spawnAgent: async (
    hostId: string,
    projectId: string,
    prompt: string,
    attachments: string[],
    model: string | null,
    effort: string | null,
    permissionMode: string | null,
    options: AgentOptions,
    /** Work handed off from another Host, for the new agent to pick up. */
    handoff: Handoff | null = null,
  ): Promise<Agent> => ({
    ...(await host<Omit<Agent, "host">>(hostId, "spawn_agent", {
      projectId,
      prompt,
      attachments,
      model,
      effort,
      permissionMode,
      options,
      handoff,
    })),
    host: hostId,
  }),
  /**
   * Put a stopped agent's committed work on the project's remote, for an agent
   * spawned on another Host to pick up. Refused while it holds uncommitted work.
   */
  handOff: (agentId: string) => host<Handoff>(on(agentId), "hand_off", { agentId }),
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
    host<Agent>(on(agentId), "send_message", {
      agentId,
      prompt,
      attachments,
      model,
      effort,
      permissionMode,
    }),
  /** Send the head of a queue that a Stop or a Fail held. */
  sendNext: (agentId: string) => host<Agent>(on(agentId), "send_next", { agentId }),
  /** Add messages to the end of a queue without sending any. */
  queueMessages: (agentId: string, messages: QueuedMessage[]) =>
    host<Agent>(on(agentId), "queue_messages", { agentId, messages }),
  removeQueued: (agentId: string, messageId: string) =>
    host<Agent>(on(agentId), "remove_queued", { agentId, messageId }),
  clearQueue: (agentId: string) => host<Agent>(on(agentId), "clear_queue", { agentId }),
  /**
   * Whether this machine's Host keeps running while the user is logged out;
   * null when it isn't a service that could.
   */
  keepRunning: () => host<boolean | null>(LOCAL, "keep_running"),
  setKeepRunning: (on: boolean) => host<void>(LOCAL, "set_keep_running", { on }),
  /** Name an agent; null hands the naming back to Claude's title. */
  renameAgent: (agentId: string, name: string | null) =>
    host<Agent>(on(agentId), "rename_agent", { agentId, name }),
  /** Tag an agent with one of `TAGS`, or untag it with null. */
  setAgentColor: (agentId: string, color: string | null) =>
    host<Agent>(on(agentId), "set_agent_color", { agentId, color }),
  /** Set how an agent's turns run, from its next one on. */
  setAgentOptions: (agentId: string, options: AgentOptions) =>
    host<Agent>(on(agentId), "set_agent_options", { agentId, options }),
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
  /**
   * Send attached files to a Host and return the paths of its copies, which
   * are what a spawn or a message then attaches. Files are read by the window,
   * where they are; the Host may be another machine.
   */
  sendAttachments: (hostId: string, paths: string[]) =>
    paths.length
      ? invoke<string[]>("send_attachments", { host: hostId, paths })
      : Promise.resolve([]),
  /** An attached image's bytes, for its thumbnail. Refused for non-images. */
  attachmentPreview: (path: string) => invoke<ArrayBuffer>("attachment_preview", { path }),
  /** A file holding the app's logo for notifications, or null where the OS finds it itself. */
  notificationIcon: () => invoke<string | null>("notification_icon"),
  stopAgent: (agentId: string) => host<void>(on(agentId), "stop_agent", { agentId }),
  discardAgent: (agentId: string) => host<void>(on(agentId), "discard_agent", { agentId }),
  agentEvents: (agentId: string) =>
    host<AgentEvent[]>(on(agentId), "agent_events", { agentId }),
  agentDiff: (agentId: string) => host<WorktreeDiff>(on(agentId), "agent_diff", { agentId }),
  agentCommit: (agentId: string, message: string) =>
    host<Commit>(on(agentId), "agent_commit", { agentId, message }),
  /** Merge into `target`, pushing it after only if `push`. */
  agentMerge: (agentId: string, target: string, push: boolean) =>
    host<MergeOutcome>(on(agentId), "agent_merge", { agentId, target, push }),
  /** Push the branch the last merge went into: not pushed then, or it failed. */
  pushMerge: (agentId: string) => host<Agent>(on(agentId), "push_merge", { agentId }),
  /**
   * Spawn a resolver for a merge of `agentId` into `target` that conflicted,
   * which was to push `target` after if `push`.
   */
  resolveConflict: async (
    agentId: string,
    target: string,
    files: string[],
    push: boolean,
    model: string | null,
    effort: string | null,
  ): Promise<Agent> => ({
    ...(await host<Omit<Agent, "host">>(on(agentId), "resolve_conflict", {
      agentId,
      target,
      files,
      push,
      model,
      effort,
    })),
    // On the conflicted agent's Host: that's where its branch is.
    host: on(agentId),
  }),
  /** Ids of merged agents whose worktree still holds work the project lacks. */
  agentsHoldingWork: (hostId: string) => host<string[]>(hostId, "agents_holding_work"),
  /** A checkout's local branches, for the merge picker. */
  projectBranches: (hostId: string, projectId: string) =>
    host<Branches>(hostId, "project_branches", { projectId }),

  /** The models the account can run: one account, so any Host can answer. */
  listModels: (hostId: string) => host<ModelInfo[]>(hostId, "list_models"),
  /** The slash commands `claude` offers in `dir`, on the Host that has it. */
  slashCommands: (hostId: string, dir: string) =>
    host<Offered>(hostId, "slash_commands", { dir }),

  /** The plugins on `hostId`, as `claude` sees them from `dir` (home if null). */
  plugins: (hostId: string, dir: string | null) =>
    host<PluginCatalog>(hostId, "plugins", { dir }),
  changePlugins: (hostId: string, dir: string | null, change: PluginChange) =>
    host<PluginOutcome>(hostId, "change_plugins", { dir, change }),

  usageSummary: (hostId: string) => host<UsageSummary>(hostId, "usage_summary"),

  startupOrphans: async (hostId: string) =>
    stamp(hostId, await host<Omit<Agent, "host">[]>(hostId, "startup_orphans")),
  dismissOrphans: (hostId: string) => host<void>(hostId, "dismiss_orphans"),
};

export type HostEvent<T> = T & { host: string };

export const events = {
  onAgentEvent: (fn: (payload: HostEvent<AgentEventPayload>) => void): Promise<UnlistenFn> =>
    listen<HostEvent<AgentEventPayload>>("agent-event", (msg) => fn(msg.payload)),

  /** An agent's record moved; it carries its Host in `host`. */
  onAgentStateChanged: (fn: (agent: Agent) => void): Promise<UnlistenFn> =>
    listen<Agent>("agent-state-changed", (msg) => fn(msg.payload)),

  onHostStatus: (fn: (status: HostEvent<HostStatus>) => void): Promise<UnlistenFn> =>
    listen<HostEvent<HostStatus>>("host-status", (msg) => fn(msg.payload)),
};
