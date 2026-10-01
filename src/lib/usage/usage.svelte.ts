/**
 * The token-usage window: whether it's up, and the numbers in it.
 *
 * The summary is read from the Agent logs on demand rather than tallied as
 * events arrive — the backend already has the whole record on disk, so asking
 * it costs one call and is right about Agents this window never opened. While
 * the window is up it re-asks on a timer, which is how a running Agent's spend
 * climbs in front of the user.
 */

import { api, type UsageSummary } from "$lib/api";
import { hosts } from "$lib/state/hosts.svelte";
import { combineUsage } from "./combine";

/** How often an open window re-reads the logs. */
export const REFRESH_MS = 8000;

/**
 * Whose tokens the tables count: every Claude Code session on this computer,
 * this app's agents, or the selected one.
 */
export type Scope = "account" | "all" | "agent";

class UsageWindow {
  open = $state(false);
  summary = $state<UsageSummary | null>(null);
  /** A read is in flight, whoever asked for it. */
  loading = $state(false);
  /** The user pressed Refresh and their read hasn't landed yet. */
  refreshing = $state(false);
  error = $state<string | null>(null);
  scope = $state<Scope>("account");
  /** The by-agent list is folded away until asked for; kept across openings. */
  agentsOpen = $state(false);

  /** The read in flight, so a second asker waits on it instead of starting another. */
  #inFlight: Promise<void> | null = null;

  toggle() {
    if (this.open) this.close();
    else this.show();
  }

  show() {
    this.open = true;
    this.load();
  }

  close() {
    this.open = false;
  }

  /** Re-read the logs, joining a read already under way rather than piling on. */
  load(): Promise<void> {
    this.#inFlight ??= this.#read();
    return this.#inFlight;
  }

  /**
   * The user's own refresh, and the only path that lights the button up. The
   * timer reads behind the numbers; a button that said "Refreshing…" every few
   * seconds on its own would read as a glitch.
   */
  async refresh() {
    this.refreshing = true;
    try {
      await this.load();
    } finally {
      this.refreshing = false;
    }
  }

  /**
   * Keeps the last good summary on screen if the read fails, so a hiccup on
   * the refresh timer doesn't blank the window.
   */
  async #read() {
    this.loading = true;
    try {
      // One account across every machine: add up what each reachable Host saw.
      const own = hosts.own;
      const reachable = hosts.list
        .map((h) => h.id)
        .filter((id) => id === own || hosts.reachable(id));
      const asked = reachable.length ? reachable : own ? [own] : [];
      this.summary = combineUsage(await Promise.all(asked.map((id) => api.usageSummary(id))));
      this.error = null;
    } catch (e) {
      this.error = String(e);
    } finally {
      this.loading = false;
      this.#inFlight = null;
    }
  }
}

export const usage = new UsageWindow();
