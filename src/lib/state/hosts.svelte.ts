/**
 * The Hosts this window talks to — this machine's, and any added by Tailscale
 * name — and where it stands with each. The Rust side keeps the connections;
 * this is the window's reading of them, kept current by `host-status` events.
 */

import { api, LOCAL, type HostInfo, type HostStatus } from "$lib/api";

class Hosts {
  list = $state<HostInfo[]>([]);

  /** Whether the window knows of any Host but its own machine's. */
  several = $derived(this.list.length > 1);

  async load() {
    try {
      this.list = await api.hosts();
    } catch {
      // The Rust side always answers; nothing to do if it somehow didn't.
    }
  }

  /** A Host's status moved. */
  update(id: string, status: HostStatus) {
    const known = this.list.find((h) => h.id === id);
    if (known) known.status = status;
    else this.list.push({ id, local: id === LOCAL, status });
  }

  status(id: string): HostStatus | undefined {
    return this.list.find((h) => h.id === id)?.status;
  }

  /** Whether calls to the Host can be answered right now. */
  reachable(id: string): boolean {
    return this.status(id)?.state === "connected";
  }

  /**
   * What to call a Host: the name it gave itself once connected, else the
   * name it was added by.
   */
  label(id: string): string {
    const s = this.status(id);
    if (s?.state === "connected") return s.name;
    return id === LOCAL ? "This machine" : id;
  }

  /** Why a Host can't be reached right now, in a few words; null if it can. */
  problem(id: string): string | null {
    const s = this.status(id);
    const name = this.label(id);
    switch (s?.state) {
      case undefined:
      case "connected":
        return null;
      case "connecting":
        return `${name} is offline`;
      case "updating":
        return `${name} is updating`;
      case "outdated":
        return `${name} runs a newer version — update this app`;
      case "behind":
        return `${name} runs v${s.version} — update it`;
      case "refused":
        return `${name} refused this window`;
    }
    return null;
  }

  async add(name: string): Promise<string | null> {
    try {
      const added = await api.addHost(name);
      this.update(added.id, added.status);
      return null;
    } catch (e) {
      return String(e);
    }
  }

  async remove(id: string): Promise<string | null> {
    try {
      await api.removeHost(id);
      this.list = this.list.filter((h) => h.id !== id);
      return null;
    } catch (e) {
      return String(e);
    }
  }
}

export const hosts = new Hosts();
