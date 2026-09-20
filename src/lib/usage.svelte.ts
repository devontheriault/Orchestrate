/**
 * The token-usage window: whether it's up, and the numbers in it.
 *
 * The summary is read from the Agent logs on demand rather than tallied as
 * events arrive — the backend already has the whole record on disk, so asking
 * it costs one call and is right about Agents this window never opened. While
 * the window is up it re-asks on a timer, which is how a running Agent's spend
 * climbs in front of the user.
 */

import { api, type UsageSummary } from "./api";

/** How often an open window re-reads the logs. */
export const REFRESH_MS = 8000;

/** Whose tokens the tables count. */
export type Scope = "all" | "agent";

class UsageWindow {
  open = $state(false);
  summary = $state<UsageSummary | null>(null);
  loading = $state(false);
  error = $state<string | null>(null);
  scope = $state<Scope>("all");

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

  /**
   * Re-read the logs. Keeps the last good summary on screen if the read fails,
   * so a hiccup on the refresh timer doesn't blank the window.
   */
  async load() {
    if (this.loading) return;
    this.loading = true;
    try {
      this.summary = await api.usageSummary();
      this.error = null;
    } catch (e) {
      this.error = String(e);
    } finally {
      this.loading = false;
    }
  }
}

export const usage = new UsageWindow();
