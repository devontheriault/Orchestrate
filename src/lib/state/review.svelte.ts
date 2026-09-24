/**
 * The selected Agent's work, as the Diff tab shows it: the diff itself, and the
 * Commit, Merge and Resolve the user can take from there.
 */

import { api, type Agent, type Branches, type Commit, type WorktreeDiff } from "$lib/api";
import type { AppStore } from "./store.svelte";

/** A Merge that hit conflicts: where it was headed and which files collided. */
export type Conflict = { target: string; files: string[] };

export class Review {
  #app: AppStore;

  /** Diff of the selected agent's worktree. Cleared when the selection moves. */
  diff = $state<WorktreeDiff | null>(null);
  loading = $state<boolean>(false);
  error = $state<string | null>(null);
  committing = $state<boolean>(false);
  merging = $state<boolean>(false);

  /** A retried push of the selected agent's last merge is out. */
  pushing = $state<boolean>(false);

  /**
   * Merges that hit conflicts, by agent: where the merge was headed and which
   * files collided. Held until the user merges again or hands it to a
   * resolver — the project was left untouched, so this is the only record of
   * what went wrong.
   */
  conflicts = $state<Record<string, Conflict>>({});

  /** A resolver is being spawned for the selected agent's conflict. */
  resolving = $state<boolean>(false);

  /**
   * The selected agent's project's mergeable branches. Null until asked for, and
   * again when the branch list can't be read — the picker then offers nothing
   * rather than guessing.
   */
  branches = $state<Branches | null>(null);

  constructor(app: AppStore) {
    this.#app = app;
  }

  /** The selected agent's name as a commit subject: what Commit offers to write. */
  get suggestedMessage(): string {
    const agent = this.#app.selectedAgent;
    const subject = (agent ? this.#app.agentName(agent) : "").replace(/\s+/g, " ").trim();
    return subject.length > 72 ? subject.slice(0, 72).trimEnd() : subject;
  }

  /** Branches that already have this work: merging into them would do nothing. */
  get mergedInto(): string[] {
    return this.diff?.merged_into ?? [];
  }

  /** What is left to merge into. */
  get targets(): string[] {
    const merged = this.mergedInto;
    return (this.branches?.names ?? []).filter((n) => !merged.includes(n));
  }

  /**
   * Where a Merge goes unless told otherwise: main — where work usually lands —
   * whatever the project happens to be checked out on. Null with nowhere left.
   */
  get defaultTarget(): string | null {
    const targets = this.targets;
    const current = this.branches?.current ?? null;
    return ["main", "master", current].find((b) => b && targets.includes(b)) ?? targets[0] ?? null;
  }

  clear() {
    this.diff = null;
    this.error = null;
    this.loading = false;
  }

  /**
   * Load the selected agent's diff. Guards against a slow response landing
   * after the user has moved to a different agent.
   */
  async load() {
    const app = this.#app;
    const id = app.selectedAgentId;
    if (!id) return;
    this.loading = true;
    this.error = null;
    try {
      const diff = await api.agentDiff(id);
      if (app.selectedAgentId !== id) return;
      this.diff = diff;
    } catch (e) {
      if (app.selectedAgentId !== id) return;
      this.error = String(e);
      this.diff = null;
    } finally {
      if (app.selectedAgentId === id) this.loading = false;
    }
  }

  /**
   * Commit everything in the selected agent's worktree, then refresh the diff.
   * The commit made, or null if it wasn't — and then `error` says why.
   */
  async commit(message: string): Promise<Commit | null> {
    const app = this.#app;
    const id = app.selectedAgentId;
    if (!id) return null;
    this.committing = true;
    this.error = null;
    try {
      const made = await api.agentCommit(id, message);
      // Committing is the other way a merged agent starts holding work the
      // project hasn't got — git status goes quiet but the tip moves.
      await app.loadHoldingWork();
      if (app.selectedAgentId === id) await this.load();
      return made;
    } catch (e) {
      if (app.selectedAgentId === id) this.error = String(e);
      return null;
    } finally {
      if (app.selectedAgentId === id) this.committing = false;
    }
  }

  /** The branches the selected agent's work could merge into. */
  async loadBranches() {
    const app = this.#app;
    const projectId = app.selectedAgent?.project_id;
    if (!projectId) {
      this.branches = null;
      return;
    }
    try {
      const branches = await api.projectBranches(projectId);
      if (app.selectedAgent?.project_id !== projectId) return;
      this.branches = branches;
    } catch {
      // Not worth an error banner: without a list the merge control just says
      // it has nothing to offer.
      if (app.selectedAgent?.project_id === projectId) this.branches = null;
    }
  }

  /**
   * Merge the selected agent's branch onto `target`. The agent survives — only
   * a discard destroys anything — so this refreshes rather than clears.
   */
  async merge(target: string) {
    const app = this.#app;
    const id = app.selectedAgentId;
    if (!id) return false;
    this.merging = true;
    this.error = null;
    try {
      const outcome = await api.agentMerge(id, target);
      if (outcome.outcome === "conflict") {
        this.conflicts[id] = { target: outcome.target, files: outcome.files };
        return false;
      }
      delete this.conflicts[id];
      // The agent now carries where it merged, and the branch list has moved on.
      await app.refresh();
      if (app.selectedAgentId === id) {
        await Promise.all([this.load(), this.loadBranches()]);
      }
      return true;
    } catch (e) {
      if (app.selectedAgentId === id) this.error = String(e);
      return false;
    } finally {
      if (app.selectedAgentId === id) this.merging = false;
    }
  }

  /**
   * Push the selected agent's last merge again, after its push failed. The
   * merge itself already stands; this only carries it to the remote.
   */
  async pushAgain() {
    const app = this.#app;
    const id = app.selectedAgentId;
    if (!id) return;
    this.pushing = true;
    try {
      app.upsert(await api.pushMerge(id));
    } catch (e) {
      if (app.selectedAgentId === id) this.error = String(e);
    } finally {
      this.pushing = false;
    }
  }

  /** Put a conflict away without resolving it. The project is untouched either way. */
  dismissConflict(agentId: string) {
    delete this.conflicts[agentId];
  }

  /**
   * Hand the selected agent's conflicted merge to a resolver: a new agent, cut
   * from this one's branch, that merges the target in and settles the
   * conflicts. When its turn completes the backend finishes the merge, and
   * both agents move to Delivered. Opens the resolver, so the user watches the
   * work that's now happening rather than the agent that's waiting on it.
   */
  async resolveConflict() {
    const app = this.#app;
    const id = app.selectedAgentId;
    const conflict = id ? this.conflicts[id] : undefined;
    if (!id || !conflict || this.resolving) return false;
    this.resolving = true;
    app.error = null;
    try {
      const resolver = await api.resolveConflict(
        id,
        conflict.target,
        conflict.files,
        app.prefs.spawnModel || null,
        app.prefs.spawnEffort || null,
      );
      delete this.conflicts[id];
      app.agents.push(resolver);
      app.selectAgent(resolver.id);
      return true;
    } catch (e) {
      app.error = String(e);
      return false;
    } finally {
      this.resolving = false;
    }
  }

  /**
   * The resolver still working on this agent's conflict, if there is one —
   * the most recent that hasn't merged yet.
   */
  resolverFor(agentId: string): Agent | null {
    let found: Agent | null = null;
    for (const a of this.#app.agents) {
      if (a.resolves?.agent_id !== agentId || a.merged_at) continue;
      if (!found || Date.parse(a.spawned_at) > Date.parse(found.spawned_at)) found = a;
    }
    return found;
  }
}
