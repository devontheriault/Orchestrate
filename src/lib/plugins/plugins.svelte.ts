/**
 * The `/plugin` window: whether it's up, the catalog in it, and the changes
 * made from it or typed as `/plugin install …`.
 *
 * Plugins belong to a machine, so the window is about one Host: the selected
 * agent's, or the one a new agent would start on. It asks `claude` from the
 * agent's worktree or the project's checkout, where a project's own plugins
 * and marketplaces are declared, and from home with no project selected.
 */

import { api, LOCAL, type PluginCatalog, type PluginChange } from "$lib/api";
import { store } from "$lib/state/store.svelte";
import { checkoutOn } from "$lib/state/projects";
import { DOING, findInstalled, type PluginRequest, type Tab } from "./catalog";

/** What a change did, or why it didn't, in words for the user. */
export type Said = { text: string; failed: boolean };

/** A change held until the user agrees to the command it would run. */
export type Confirming = {
  change: PluginChange;
  /** The plugin it's about, for the busy mark. */
  key: string;
  message: string;
  command: string;
  sha256: string;
};

type Reading = { at: string; done: Promise<void> };

class Plugins {
  open = $state(false);
  tab = $state<Tab>("discover");
  query = $state("");
  catalog = $state<PluginCatalog | null>(null);
  loading = $state(false);
  error = $state<string | null>(null);
  /** Changes under way: what's being done, by the plugin id or marketplace name. */
  busy = $state<Record<string, string>>({});
  /** What the last change made in the window did, until the next. */
  notice = $state<Said | null>(null);
  confirming = $state<Confirming | null>(null);
  /** The Host and directory the catalog is about, fixed when the window opens. */
  host = $state(LOCAL);
  dir = $state<string | null>(null);

  /** The read in flight and the place it's of, so a second asker waits on it. */
  #reading: Reading | null = null;

  show(tab: Tab = "discover", query = "") {
    this.#locate();
    this.tab = tab;
    this.query = query;
    this.notice = null;
    this.open = true;
    this.load();
  }

  close() {
    this.open = false;
    this.confirming = null;
  }

  /**
   * Re-read the catalog, joining a read of the same place already under way
   * unless `fresh` — a read begun before a change may not show it. The last
   * catalog stays up meanwhile, so a refresh never blanks it.
   */
  load(fresh = false): Promise<void> {
    const at = this.#where();
    if (fresh || this.#reading?.at !== at) {
      const reading: Reading = { at, done: Promise.resolve() };
      this.#reading = reading;
      reading.done = this.#read(reading);
    }
    return this.#reading!.done;
  }

  /** Only the latest read may land: an earlier one may be of another place, or stale. */
  async #read(reading: Reading) {
    this.loading = true;
    try {
      const catalog = await api.plugins(this.host, this.dir);
      if (this.#reading === reading) {
        this.catalog = catalog;
        this.error = null;
      }
    } catch (e) {
      if (this.#reading === reading) this.error = String(e);
    } finally {
      if (this.#reading === reading) {
        this.#reading = null;
        this.loading = false;
      }
    }
  }

  /**
   * Make a change, marked busy against `key` while it runs. A command the
   * plugin's marketplace wants run is held for the user to see, in the window.
   */
  async change(key: string, verb: string, change: PluginChange): Promise<Said | null> {
    if (this.busy[key]) return null;
    this.busy = { ...this.busy, [key]: verb };
    try {
      const outcome = await api.changePlugins(this.host, this.dir, change);
      if (outcome.outcome === "confirm") {
        this.confirming = { change, key, ...outcome };
        this.open = true;
        return null;
      }
      // Still busy until the lists show the change: a quick read.
      await this.load(true);
      return { text: outcome.message, failed: false };
    } catch (e) {
      return { text: String(e), failed: true };
    } finally {
      const { [key]: _, ...rest } = this.busy;
      this.busy = rest;
    }
  }

  /** The same change again, agreeing to the command it showed. */
  async accept(): Promise<Said | null> {
    const held = this.confirming;
    if (!held) return null;
    this.confirming = null;
    const change = { ...held.change, acceptCommand: held.sha256 } as PluginChange;
    return this.change(held.key, held.change.do === "update" ? "Updating" : "Installing", change);
  }

  /** Make a change from the window, and say how it went at the window's foot. */
  async act(key: string, verb: string, change: PluginChange) {
    const said = await this.change(key, verb, change);
    if (said) this.notice = said;
  }

  /**
   * Do what a typed `/plugin …` asks. Opening the window says nothing; a
   * change says how it went, for the composer to show.
   */
  async ask(request: PluginRequest): Promise<Said | null> {
    if ("open" in request) {
      this.show(request.open, request.query);
      return null;
    }
    this.#locate();
    if ("marketplace" in request) {
      switch (request.marketplace) {
        case "add":
          return this.change(request.source, "Adding", { do: "add_marketplace", source: request.source });
        case "remove":
          return this.change(request.name, "Removing", { do: "remove_marketplace", name: request.name });
        case "update":
          return this.change(request.name ?? "*", "Updating", {
            do: "update_marketplace",
            ...(request.name ? { name: request.name } : {}),
          });
      }
    }
    if (request.plugin === "install") {
      return this.change(request.id, "Installing", { do: "install", plugin: request.id });
    }
    // The rest act on an installed plugin, in the scope it was installed in.
    if (!this.catalog) await this.load();
    const found = findInstalled(this.catalog?.installed ?? [], request.id);
    if (!found) {
      return { text: `No installed plugin called “${request.id}”. Type /plugin installed to see them.`, failed: true };
    }
    return this.change(found.id, DOING[request.plugin], {
      do: request.plugin,
      plugin: found.id,
      scope: found.scope,
    });
  }

  /** Where the selected agent, or the agent about to be spawned, would run. */
  #locate() {
    const agent = store.selectedAgent;
    const project = store.selectedProject;
    const host = agent ? agent.host : store.draftHost;
    const dir = agent ? agent.worktree_path : (project && checkoutOn(project, host)?.path) || null;
    if (host === this.host && dir === this.dir) return;
    this.host = host;
    this.dir = dir;
    // Another machine's, or another project's: the old list isn't this one.
    this.catalog = null;
    this.error = null;
  }

  #where() {
    return `${this.host}:${this.dir ?? "~"}`;
  }
}

export const plugins = new Plugins();

