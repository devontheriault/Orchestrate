/**
 * The app's state: its Projects and Agents, what's selected, and the
 * lifecycle verbs — Spawn, Resume, Stop, Discard — the UI calls on them.
 *
 * Three slices hang off it, each in its own file because each is its own
 * concern that happens to need the Agents and the selection:
 *
 * - `review` — the selected Agent's diff, and the Commit / Merge / Resolve
 *   taken from the Diff tab (`review.svelte.ts`).
 * - `queue` — messages lined up behind a working Agent (`queue.svelte.ts`).
 * - `prefs` — the Model, Effort, Mode and options a new Agent opens on
 *   (`prefs.svelte.ts`).
 *
 * The model list is its own store (`models.svelte.ts`): it needs nothing here.
 */

import {
  api,
  events,
  LOCAL,
  type Agent,
  type AgentEvent,
  type AgentOptions,
  type HostStatus,
  type Project,
} from "$lib/api";
import { space } from "$lib/spaces/space.svelte";
import { notify } from "$lib/notify/notify";
import { turnNotice } from "$lib/notify/turnNotice";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { SvelteMap } from "svelte/reactivity";
import { readJson, writeJson } from "$lib/storage";
import { hosts } from "./hosts.svelte";
import { models } from "./models.svelte";
import {
  canSpawnOn,
  checkoutOn,
  defaultHost,
  groupKey,
  groupProjects,
  orderProjects,
  type ProjectGroup,
} from "./projects";
import { TurnPrefs } from "./prefs.svelte";
import { Queue } from "./queue.svelte";
import { Review } from "./review.svelte";

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
 * Only logged events from the first live one's millisecond on can be among
 * them, so only those are keyed: keying means stringifying, and the rest of a
 * long log can run to megabytes.
 */
function mergeEvents(logged: AgentEvent[], live: AgentEvent[]): AgentEvent[] {
  if (live.length === 0) return logged;
  const from = live.reduce((min, e) => Math.min(min, Date.parse(e.ts)), Infinity);
  const seen = new Set(logged.filter((e) => !(Date.parse(e.ts) < from)).map(eventKey));
  return [...logged, ...live.filter((e) => !seen.has(eventKey(e)))];
}

/**
 * Where the user was, stashed across a reload. sessionStorage is scoped to this
 * webview session, so it survives Ctrl+Alt+R and is gone on the next launch.
 */
const RESUME_KEY = "cw:resume-selection";

type Selection = { project: string | null; agent: string | null };

/**
 * What each other machine's Host last said it had, so its agents stay listed
 * — dimmed, as last seen — while it's off, and across a restart of this
 * window. The Host's own disk is the only record; this is just a cache.
 */
const HOST_CACHE_KEY = "cw:host-cache";

type HostData = { projects: Project[]; agents: Agent[]; orphans: Agent[]; holding: string[] };

const noHostData = (): HostData => ({ projects: [], agents: [], orphans: [], holding: [] });

/** The order the user dragged the projects into, as project ids. */
const PROJECT_ORDER_KEY = "cw:project-order";

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

export class AppStore {
  /** Projects as the window shows them: each one's checkouts on every Host. */
  projects = $state<ProjectGroup[]>([]);
  agents = $state<Agent[]>([]);
  orphans = $state<Agent[]>([]);
  /**
   * Each Agent's log, as it arrived. Events are never changed once they're
   * in, so they're held as plain objects rather than deep state: proxying
   * every tool result made each new event cost a walk of the whole log. A
   * new event replaces its Agent's array, so readers see it as a change.
   */
  eventsByAgent = new SvelteMap<string, AgentEvent[]>();

  selectedProjectId = $state<string | null>(null);
  selectedAgentId = $state<string | null>(null);

  /**
   * Which projects are showing their agents in the sidebar. A project opens
   * when it's selected and closes when its row is clicked again, so the tree
   * only ever holds the lists the user asked to see.
   */
  expandedProjects = $state<Record<string, boolean>>({});

  /**
   * The order the user dragged the projects into. Kept in the window rather
   * than on a Host: a project can span several Hosts, and the list is the
   * window's. Empty until the first drag, so the Hosts' order stands till then.
   */
  private projectOrder = readJson<string[]>(PROJECT_ORDER_KEY, []);

  /**
   * Buckets the user has opened or folded in the sidebar, by project and then
   * by bucket. Unset means the bucket's default. Kept here rather than in the
   * list, so collapsing the project or opening the rail's flyout shows the
   * tree as the user left it.
   */
  openBuckets = $state<Record<string, Record<string, boolean>>>({});

  /**
   * Leads whose Helpers the user has folded away in the sidebar, by the Lead's
   * id. Unset means open. Kept here for the same reason as `openBuckets`.
   */
  foldedTeams = $state<Record<string, boolean>>({});

  /** Which body the detail pane shows for the selected agent. */
  detailTab = $state<"output" | "diff">("output");

  /**
   * Merged agents whose worktree still holds work the project doesn't have.
   * Read from git rather than from the agent record, because a merge record
   * only says what once happened — it can't know the agent was resumed and
   * wrote more since. Empty until the first read lands, so an agent reads as
   * Delivered until git says otherwise rather than flickering on startup.
   */
  holdingWork = $state<string[]>([]);
  private holding = $derived(new Set(this.holdingWork));

  /**
   * Clears of Delivered in progress, by project. Kept here rather than in the
   * list that starts one, so collapsing the project or closing the flyout mid-run
   * doesn't lose the progress. `total` is 0 until git has said which are safe.
   */
  bulkClears = $state<Record<string, { done: number; total: number }>>({});

  /** A follow-up prompt is in flight for the selected agent. */
  sending = $state<boolean>(false);

  /**
   * A new agent being composed. The detail pane shows a blank output page with
   * an empty transcript and the composer waiting for the opening prompt —
   * the same surface the agent's own output will fill, rather than a dialog
   * over the top of it. Mutually exclusive with having an agent selected.
   */
  drafting = $state<boolean>(false);

  /** The opening prompt is in flight; the draft page is waiting on a spawn. */
  spawning = $state<boolean>(false);

  orphanBannerDismissed = $state<boolean>(false);
  error = $state<string | null>(null);

  /**
   * The Host a new agent in the selected project goes to, if the user picked
   * one on the draft page; null leaves it to `draftHost`'s default.
   */
  pickedHost = $state<string | null>(null);

  /** Each Host last connected to, to tell a reconnection from a successor. */
  private hostInstances: Record<string, string> = {};

  /** What each Host last listed. See `HOST_CACHE_KEY`. */
  private perHost = readJson<Record<string, HostData>>(HOST_CACHE_KEY, {});

  /** Each Host's own project ids, as `host/id`, to the project they're part of. */
  private groupOf = new Map<string, string>();

  /** Projects an event mentioned before this window had heard of them. */
  private unheardOf = new Set<string>();

  /**
   * Whether `refresh` has ever succeeded. Until it has, an empty list means
   * "not heard yet" rather than "no projects", and the sidebar says nothing.
   */
  loaded = $state(false);

  /** The read `refresh` has out, and whether another was asked for meanwhile. */
  private refreshing: Promise<void> | null = null;
  private refreshAgain = false;

  /** The selected agent's diff, and what the Diff tab does with it. */
  readonly review = new Review(this);

  /** Messages waiting behind each agent. */
  readonly queue = new Queue(this);

  /** The picks a new agent opens on. */
  readonly prefs = new TurnPrefs(this);

  /** Agents whose on-disk log has been replayed into `eventsByAgent`. */
  private hydrated = new Set<string>();

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
    return this.holding.has(agentId);
  }

  /**
   * Whether an agent sits in the sidebar's Delivered bucket: merged, not
   * working, and holding nothing the project hasn't got.
   */
  isDelivered(a: Agent): boolean {
    return a.state !== "running" && !!a.merged_at && !this.isHoldingWork(a.id);
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

  selectedProject = $derived(
    this.selectedProjectId
      ? this.projects.find((p) => p.id === this.selectedProjectId) ?? null
      : null,
  );

  /** The project an agent is filed under. */
  projectOf(agent: Agent): ProjectGroup | null {
    return this.projects.find((p) => p.id === agent.project_id) ?? null;
  }

  /** Where a new agent in the selected project would start. */
  draftHost = $derived.by(() => {
    const group = this.selectedProject;
    if (!group) return hosts.home;
    // A pick that has since gone offline gives way to the default, not least
    // because the picker hides when only one machine is left online.
    const picked = this.pickedHost;
    if (picked && canSpawnOn(group, picked) && hosts.reachable(picked)) return picked;
    return defaultHost(group, this.prefs.hosts[group.id], (h) => hosts.reachable(h), hosts.own);
  });

  eventsForSelected = $derived(
    this.selectedAgentId ? this.eventsByAgent.get(this.selectedAgentId) ?? [] : [],
  );

  /**
   * Whether the selected agent's conversation can be picked back up. Needs a
   * session to resume and a worktree to run in — the backend has the last word
   * on the worktree, since only it can see the disk.
   */
  canContinue = $derived.by(() => {
    const a = this.selectedAgent;
    return !!a && a.state !== "running" && !!a.session_id && hosts.reachable(a.host);
  });

  async start() {
    if (this.started) return;
    this.started = true;

    this.unlisteners.push(hosts.onConnect((id, instance) => this.onConnect(id, instance)));
    // Listening before reading, so nothing that happens between the two is
    // missed. The three are independent, so they're asked for together: each
    // is a round trip, and the window is empty until the read after them.
    this.unlisteners.push(
      ...(await Promise.all([
        events.onAgentEvent(({ agent_id, event }) => {
          this.eventsByAgent.set(agent_id, [...(this.eventsByAgent.get(agent_id) ?? []), event]);
        }),
        events.onHostStatus(({ host, ...status }) => hosts.update(host, status as HostStatus)),
        events.onAgentStateChanged((raw) => {
          const agent = this.ingest(raw);
          this.catchUpOn(agent);
          const before = this.agents.find((a) => a.id === agent.id);
          this.upsert(agent);
          const notice = turnNotice(before, agent, this.agentName(agent));
          if (notice) notify(notice, agent.id === this.selectedAgentId);
          // An agent that just exited has a final diff worth showing.
          if (agent.id === this.selectedAgentId && this.detailTab === "diff") {
            this.review.load();
          }
          // A merged agent's worktree only changes because the agent ran, so a
          // turn ending is the one moment its bucket can have moved. That is why
          // nothing here polls git on a timer.
          if (agent.state !== "running" && agent.merged_at) this.loadHoldingWork();
        }),
      ])),
    );
    await hosts.load();

    // A phone in the background is suspended, and a laptop asleep, and either
    // can come back to a connection that died meanwhile without a word. The
    // links find out by themselves within half a minute; coming back on
    // screen, or onto a network, they check at once.
    const wake = () => {
      if (document.visibilityState === "visible") hosts.wake();
    };
    document.addEventListener("visibilitychange", wake);
    window.addEventListener("online", wake);
    this.unlisteners.push(() => {
      document.removeEventListener("visibilitychange", wake);
      window.removeEventListener("online", wake);
    });

    // Not awaited: the model list only fills a picker, and blocking the first
    // paint on a network round-trip would be a poor trade.
    models.load();
    await this.refresh();
  }

  /**
   * What to call an agent: the Title the user gave it, else the one Claude
   * wrote for it, or — until that lands, and on agents from before titles
   * existed — the first line of the prompt it was given.
   */
  agentName(a: Agent): string {
    const name = a.user_title?.trim() || a.title?.trim();
    if (name) return name;
    return a.task.prompt.split("\n")[0] ?? "";
  }

  async stop() {
    for (const un of this.unlisteners) un();
    this.unlisteners = [];
    this.started = false;
  }

  /**
   * Re-read everything from the Host. Reads never overlap: one asked for while
   * another is out runs once that one is back, so an older answer can't land
   * on top of a newer one — as it would when a window starts on a Host that
   * is just making way for the next.
   */
  refresh(): Promise<void> {
    if (this.refreshing) {
      this.refreshAgain = true;
      return this.refreshing;
    }
    this.refreshing = (async () => {
      do {
        this.refreshAgain = false;
        await this.load();
      } while (this.refreshAgain);
      // After the last read rather than after each, so the queues it hands
      // over aren't overwritten by a read that started before they arrived.
      if (this.loaded) this.queue.handOver();
    })().finally(() => (this.refreshing = null));
    return this.refreshing;
  }

  /**
   * Read everything from every Host that can answer. This machine's Host is
   * always asked, where there is one — it starts if it isn't running. Another machine's is asked
   * only while connected; until then its agents stay as it last listed them.
   */
  private async load() {
    this.error = null;
    const own = hosts.own;
    const ids = this.hostIds();
    await Promise.all(
      this.reachableHosts().map(async (id) => {
        try {
          const [projects, agents, orphans, holding] = await Promise.all([
            api.listProjects(id),
            api.listAgents(id),
            api.startupOrphans(id),
            api.agentsHoldingWork(id),
          ]);
          this.perHost[id] = { projects, agents, orphans, holding };
          if (id === own) this.loaded = true;
        } catch (e) {
          if (id === own) this.error = String(e);
        }
      }),
    );
    // On a phone there's no Host of its own to wait for: what the others
    // could answer is everything there is.
    if (!own) this.loaded = true;
    // A Host the user removed takes its agents with it.
    for (const id of Object.keys(this.perHost)) {
      if (!ids.has(id)) delete this.perHost[id];
    }
    this.rebuild();
    this.applyInitialSelection();
  }

  /** Every Host the window knows of: this machine's, where there is one, and the added ones. */
  private hostIds(): Set<string> {
    const own = hosts.own;
    return new Set([...(own ? [own] : []), ...hosts.list.map((h) => h.id)]);
  }

  /** The Hosts worth asking now: this machine's always, the others while connected. */
  private reachableHosts(): string[] {
    return [...this.hostIds()].filter((id) => id === hosts.own || hosts.reachable(id));
  }

  /** Put the Hosts' lists together into what the window shows. */
  private rebuild() {
    const all = Object.values(this.perHost);
    this.projects = orderProjects(
      groupProjects(
        all.flatMap((d) => d.projects),
        LOCAL,
      ),
      this.projectOrder,
    );
    this.groupOf = new Map(
      this.projects.flatMap((g) => g.checkouts.map((c) => [`${c.host}/${c.id}`, g.id])),
    );
    this.agents = all.flatMap((d) => d.agents).map((a) => this.ingest(a));
    this.orphans = all.flatMap((d) => d.orphans).map((a) => this.ingest(a));
    this.holdingWork = all.flatMap((d) => d.holding);
    this.saveHostCache();
  }

  private saveHostCache() {
    const remote = Object.fromEntries(
      Object.entries(this.perHost).filter(([id]) => id !== LOCAL),
    );
    writeJson(HOST_CACHE_KEY, remote);
  }

  /**
   * An agent as the window keeps it: routed to its Host, and filed under the
   * project as the window groups them rather than its Host's own project id.
   */
  private ingest(a: Agent): Agent {
    api.route(a);
    const home = a.home_project_id ?? a.project_id;
    const group = this.groupOf.get(`${a.host}/${home}`);
    return { ...a, home_project_id: home, project_id: group ?? `${a.host}/${home}` };
  }

  /**
   * An event about an agent in a project this window hasn't heard of —
   * registered from another window, or cloned onto a Host for a spawn made
   * elsewhere — means its lists are behind: read them again, once per
   * project. (An agent whose project was since removed is filed on its own
   * for good, and mustn't set off a read every time.)
   */
  private catchUpOn(agent: Agent) {
    const key = `${agent.host}/${agent.home_project_id}`;
    if (this.groupOf.has(key) || this.unheardOf.has(key) || !this.loaded) return;
    this.unheardOf.add(key);
    this.refresh();
  }

  /**
   * Follow the window's connection to a Host. Every reconnection means
   * re-reading everything, to the same Host as much as to the one that
   * replaced it: whatever the agents did in between reached the logs but not
   * this window. So does reaching another machine's Host for the first time.
   */
  private onConnect(id: string, instance: string) {
    // On a phone the model list waits on whichever Host answers first.
    if (models.error) models.load();
    const previous = this.hostInstances[id];
    this.hostInstances[id] = instance;
    // A new local Host adopted its own Orphans; the banner is about those now.
    if (id === LOCAL && previous !== undefined && previous !== instance) {
      this.orphanBannerDismissed = false;
    }
    // The first connection to this machine's Host is the one `start` is
    // already loading from, unless that load gave up waiting for it.
    if (id !== LOCAL || previous !== undefined || !this.loaded) this.resync();
  }

  private async resync() {
    this.hydrated.clear();
    await this.refresh();
    if (this.selectedAgentId) this.hydrateEvents(this.selectedAgentId);
  }

  /**
   * Re-ask git which merged agents are holding work. Swallows failures: the
   * previous answer is a better sidebar than an error banner over a reading.
   */
  async loadHoldingWork() {
    try {
      this.holdingWork = await this.readHoldingWork();
    } catch {
      // Keep the last answer.
    }
  }

  /** Ask every reachable Host which of its merged agents hold work. */
  private async readHoldingWork(): Promise<string[]> {
    const reachable = this.reachableHosts();
    const answers = await Promise.all(reachable.map((id) => api.agentsHoldingWork(id)));
    for (const [i, id] of reachable.entries()) {
      if (this.perHost[id]) this.perHost[id].holding = answers[i];
    }
    return Object.values(this.perHost).flatMap((d) => d.holding);
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

  /** Register a folder on this machine as a project, or as another checkout of one. */
  async addProject(name: string, path: string, setUp: boolean) {
    try {
      const p = await api.addProject(name, path, setUp);
      (this.perHost[LOCAL] ??= noHostData()).projects.push(p);
      this.rebuild();
      this.selectProject(groupKey(p));
    } catch (e) {
      this.error = String(e);
    }
  }

  /** Put the projects in the order given, as ids, and remember it. */
  reorderProjects(ids: string[]) {
    this.projectOrder = ids;
    this.projects = orderProjects(this.projects, ids);
    writeJson(PROJECT_ORDER_KEY, ids);
  }

  /**
   * Unregister a project on every Host that can be reached, and say which
   * couldn't be — it's still registered there until removed from there too.
   * Its agents and checkouts stay until they're discarded.
   */
  async removeProject(id: string) {
    const group = this.projects.find((p) => p.id === id);
    if (!group) return;
    const missed: string[] = [];
    for (const c of group.checkouts) {
      if (c.host !== LOCAL && !hosts.reachable(c.host)) {
        missed.push(hosts.label(c.host));
        continue;
      }
      try {
        await api.removeProject(c.host, c.id);
        const data = this.perHost[c.host];
        if (data) data.projects = data.projects.filter((p) => p.id !== c.id);
      } catch (e) {
        missed.push(`${hosts.label(c.host)} (${e})`);
      }
    }
    this.rebuild();
    if (missed.length) {
      this.error = `Still registered on ${missed.join(", ")}: remove it there once it can be reached.`;
    }
    if (this.projects.some((p) => p.id === id)) return;
    delete this.expandedProjects[id];
    delete this.openBuckets[id];
    if (this.selectedProjectId === id) {
      this.selectedProjectId = null;
      this.selectAgent(null);
      const next = this.projects[0]?.id;
      if (next) this.selectProject(next);
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
    this.review.clear();
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
    if (project !== this.selectedProjectId) this.pickedHost = null;
    this.selectedProjectId = project;
    this.expandedProjects[project] = true;
    this.selectedAgentId = null;
    this.review.clear();
    this.detailTab = "output";
    this.drafting = true;
  }

  cancelDraft() {
    this.drafting = false;
  }

  /**
   * Leave the agent, or the draft, for the list it was opened from: a phone's
   * way back, where the two take turns on one screen.
   */
  closeDetail() {
    if (this.drafting) this.cancelDraft();
    else this.selectAgent(null);
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
      this.eventsByAgent.set(id, mergeEvents(logged, this.eventsByAgent.get(id) ?? []));
    } catch (e) {
      // Leave it unhydrated so re-selecting the Agent tries again.
      this.hydrated.delete(id);
      this.error = String(e);
    }
  }

  showTab(tab: "output" | "diff") {
    this.detailTab = tab;
    // Always re-read on entry: the worktree may have moved since last time.
    if (tab === "diff" && !this.review.loading) this.review.load();
  }

  /**
   * Bring an Agent on screen wherever it lives: switch to its Project, then
   * select it. False if there is no such Agent any more — its log can outlive
   * it, so a caller working from the logs can be holding a Discarded id.
   */
  showAgent(id: string): boolean {
    const agent = this.agents.find((a) => a.id === id);
    if (!agent) return false;
    this.goTo(agent);
    return true;
  }

  jumpToFirstOrphan() {
    if (this.orphans.length === 0) return;
    this.goTo(this.orphans[0]);
    this.dismissOrphans();
  }

  /** Open the Agents Space on `agent`, under its project if that's still listed. */
  private goTo(agent: Agent) {
    space.show("agents");
    if (this.projects.some((p) => p.id === agent.project_id)) {
      this.selectedProjectId = agent.project_id;
      this.expandedProjects[agent.project_id] = true;
    }
    this.selectAgent(agent.id);
  }

  /** Hide the Orphan banner, and tell the Hosts so a reload keeps it hidden. */
  dismissOrphans() {
    this.orphanBannerDismissed = true;
    for (const id of new Set(this.orphans.map((o) => o.host))) {
      api.dismissOrphans(id).catch(() => {});
    }
  }

  /**
   * Start a new agent on the selected project, with any files attached to its
   * prompt. `model` is the picker's value; empty means no `--model` at all.
   * Returns whether it started — the draft page keeps the prompt on failure so
   * the user can retry rather than retype.
   *
   * `mail` says the prompt holds mail: the agent is locked to reading and
   * suggesting for good, and `mode` is ignored (ADR 0018).
   */
  async spawn(
    prompt: string,
    attachments: string[],
    model: string,
    effort: string,
    mode: string,
    { mail = false }: { mail?: boolean } = {},
  ): Promise<boolean> {
    const group = this.selectedProject;
    if (!group || !prompt.trim()) return false;
    const host = this.draftHost;
    return this.startAgent(group, host, [model, effort, mode], async (checkout) =>
      mail
        ? api.spawnMailAgent(host, checkout, prompt, model || null, effort || null, this.prefs.options)
        : api.spawnAgent(
            host,
            checkout,
            prompt,
            await api.sendAttachments(host, attachments),
            model || null,
            effort || null,
            mode || null,
            this.prefs.options,
          ),
    );
  }

  /**
   * Take the selected agent's work to `host` (ADR 0013): its own Host puts
   * what it committed on the project's remote, and a new agent on `host` picks
   * it up with `prompt`. Returns whether it started, and opens it if it did.
   */
  async handOff(
    host: string,
    prompt: string,
    attachments: string[],
    model: string,
    effort: string,
    mode: string,
  ): Promise<boolean> {
    const from = this.selectedAgent;
    const group = from && this.projectOf(from);
    if (!from || !group || !prompt.trim()) return false;
    return this.startAgent(group, host, [model, effort, mode], async (checkout) => {
      const handoff = await api.handOff(from.id);
      return api.spawnAgent(
        host,
        checkout,
        prompt,
        await api.sendAttachments(host, attachments),
        model || null,
        effort || null,
        mode || null,
        from.options ?? this.prefs.options,
        handoff,
      );
    });
  }

  /**
   * Start an agent in `group` on `host` with `start`, given the checkout to
   * start it in, cloning the project onto `host` first if it has none there.
   * Opens it if it started, and remembers the picks it started on.
   */
  private async startAgent(
    group: ProjectGroup,
    host: string,
    [model, effort, mode]: [string, string, string],
    start: (checkout: string) => Promise<Agent>,
  ): Promise<boolean> {
    if (this.spawning) return false;
    this.spawning = true;
    this.error = null;
    try {
      const checkout = checkoutOn(group, host) ?? (await this.cloneOnto(group, host));
      const agent = this.ingest(await start(checkout.id));
      this.prefs.remember(model, effort, mode);
      this.prefs.rememberHost(group.id, host);
      this.upsert(agent);
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
   * Have `host` clone a project it has no checkout of, so an agent can start
   * there. Only a project with a remote can be; the rest live on one Host.
   */
  private async cloneOnto(group: ProjectGroup, host: string): Promise<Project> {
    if (!group.remote) {
      throw new Error(`${group.name} has no remote, so it can only run on ${hosts.label(group.checkouts[0]?.host ?? LOCAL)}`);
    }
    const cloned = await api.cloneProject(host, group.name, group.remote);
    (this.perHost[host] ??= noHostData()).projects.push(cloned);
    this.rebuild();
    return cloned;
  }

  /**
   * Say something to the selected agent, with any files attached to it, on
   * `model` and in `mode`. The Host puts it back to work in the worktree it
   * already has if it's free, and queues it behind the Turn in flight if not.
   * Returns whether the Host took it.
   */
  async resume(
    prompt: string,
    attachments: string[],
    model: string,
    effort: string,
    mode: string,
  ) {
    const id = this.selectedAgentId;
    const on = this.selectedAgent?.host ?? LOCAL;
    if (!id || !prompt.trim() || this.sending) return false;
    this.sending = true;
    this.error = null;
    try {
      const agent = await api.sendMessage(
        id,
        prompt,
        await api.sendAttachments(on, attachments),
        model || null,
        effort || null,
        mode || null,
      );
      this.prefs.remember(model, effort, mode);
      this.take(agent);
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

  /** Take an agent's record as a Host sent it: filed as the window files them. */
  take(raw: Agent) {
    this.upsert(this.ingest(raw));
  }

  /**
   * Take an agent's latest record, adding it if this window hadn't seen it.
   * `agent` must already be filed as the window files them (`ingest`).
   */
  private upsert(agent: Agent) {
    const i = this.agents.findIndex((a) => a.id === agent.id);
    if (i >= 0) this.agents[i] = agent;
    else this.agents.push(agent);
  }

  /**
   * Name an agent, or with null let Claude's title name it again. Returns
   * whether it took. Allowed while the agent works, as are the edits below.
   */
  renameAgent(id: string, name: string | null): Promise<boolean> {
    return this.edit(api.renameAgent(id, name));
  }

  /** Tag an agent with a colour, or untag it with null. */
  setAgentColor(id: string, color: string | null): Promise<boolean> {
    return this.edit(api.setAgentColor(id, color));
  }

  /**
   * Set how an agent's turns run from its next one on — and, since options
   * are meant to stick, what the next new agent spawns with.
   */
  async setAgentOptions(id: string, options: AgentOptions): Promise<boolean> {
    const done = await this.edit(api.setAgentOptions(id, options));
    if (done) this.prefs.rememberOptions(options);
    return done;
  }

  /**
   * Wait out an edit. The record it returns isn't put in place: the backend
   * announces the edit too, in order with the agent's other changes, and a
   * reply that lands after a Turn's end was announced would put the agent
   * back to work on screen.
   */
  private async edit(editing: Promise<Agent>): Promise<boolean> {
    try {
      await editing;
      return true;
    } catch (e) {
      this.error = String(e);
      return false;
    }
  }

  async stopAgent(id: string) {
    try {
      await api.stopAgent(id);
    } catch (e) {
      this.error = String(e);
    }
  }

  async discardAgent(id: string) {
    try {
      await api.discardAgent(id);
      this.agents = this.agents.filter((a) => a.id !== id);
      this.eventsByAgent.delete(id);
      this.hydrated.delete(id);
      if (this.selectedAgentId === id) {
        this.selectedAgentId = null;
        this.review.clear();
      }
    } catch (e) {
      this.error = String(e);
    }
  }

  /**
   * Clear a project's Delivered bucket by Discarding each agent in it. Git is
   * asked afresh first rather than trusting `holdingWork`, which is only
   * re-read when a turn ends or a commit lands and reads empty before its
   * first answer — a stale "nothing held" here would destroy work. One at a
   * time, since each discard removes a worktree and deletes a branch in the
   * same repository.
   */
  async clearDelivered(projectId: string) {
    if (this.bulkClears[projectId]) return;
    this.bulkClears[projectId] = { done: 0, total: 0 };
    try {
      this.holdingWork = await this.readHoldingWork();
      const delivered = this.agentsForProject(projectId).filter((a) => this.isDelivered(a));
      this.bulkClears[projectId].total = delivered.length;
      for (const a of delivered) {
        await this.discardAgent(a.id);
        this.bulkClears[projectId].done++;
      }
    } catch (e) {
      this.error = String(e);
    } finally {
      delete this.bulkClears[projectId];
    }
  }
}

export const store = new AppStore();
