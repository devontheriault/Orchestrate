/**
 * The `/plugin` window, as logic: what a search finds, how a plugin's id and
 * install count read, and what a typed `/plugin …` asks for.
 */

import type { AvailablePlugin, InstalledPlugin } from "$lib/api";

/** The window's three lists, named as Claude Code's `/plugin` names them. */
export type Tab = "discover" | "installed" | "marketplaces";

/**
 * The plugins worth showing for `query`, best first: names that start with
 * it, then names that contain it, then plugins whose description or
 * marketplace mentions it. Otherwise in the order given — most installed
 * first, as the catalog comes.
 */
export function matchPlugins<P extends AvailablePlugin | InstalledPlugin>(
  plugins: P[],
  query: string,
): P[] {
  const q = query.trim().toLowerCase();
  if (!q) return plugins;
  const rank = (p: P): number => {
    const { name, marketplace } = splitId(p.id);
    const n = name.toLowerCase();
    if (n.startsWith(q)) return 0;
    if (n.includes(q)) return 1;
    if (p.description.toLowerCase().includes(q) || marketplace.toLowerCase().includes(q)) return 2;
    return -1;
  };
  return plugins
    .map((p, i) => ({ p, i, r: rank(p) }))
    .filter(({ r }) => r >= 0)
    .sort((a, b) => a.r - b.r || a.i - b.i)
    .map(({ p }) => p);
}

/** `name@marketplace`, apart. An id with no `@` is all name. */
export function splitId(id: string): { name: string; marketplace: string } {
  const at = id.lastIndexOf("@");
  return at <= 0 ? { name: id, marketplace: "" } : { name: id.slice(0, at), marketplace: id.slice(at + 1) };
}

/** "70.3k installs": rounded, since the point is which plugins are popular. */
export function installsLabel(n: number | null): string {
  if (n === null) return "";
  const count =
    n >= 1_000_000
      ? `${(n / 1_000_000).toFixed(1)}M`
      : n >= 10_000
        ? `${Math.round(n / 1000)}k`
        : n >= 1000
          ? `${(n / 1000).toFixed(1)}k`
          : `${n}`;
  return `${count.replace(/\.0(?=[kM])/, "")} install${n === 1 ? "" : "s"}`;
}

/**
 * The installed plugin `named` means: its full id, or its name alone when
 * only one installed plugin has that name.
 */
export function findInstalled(installed: InstalledPlugin[], named: string): InstalledPlugin | null {
  const exact = installed.find((p) => p.id === named);
  if (exact) return exact;
  const byName = installed.filter((p) => splitId(p.id).name === named);
  return byName.length === 1 ? byName[0] : null;
}

/** What each change to a plugin is called while it runs. */
export const DOING = {
  install: "Installing",
  uninstall: "Uninstalling",
  enable: "Enabling",
  disable: "Disabling",
  update: "Updating",
} as const;

/** What a typed `/plugin …` asks for. */
export type PluginRequest =
  /** Show the window on a list, searching it for `query`. */
  | { open: Tab; query: string }
  | { plugin: keyof typeof DOING; id: string }
  | { marketplace: "add"; source: string }
  | { marketplace: "remove"; name: string }
  /** Every marketplace, with no name. */
  | { marketplace: "update"; name: string | null };

/**
 * What the words after `/plugin` ask for, in Claude Code's own forms:
 * `install <plugin>`, `marketplace add <source>` and the rest. Anything else is
 * a search of what's on offer, so `/plugin github` finds the GitHub plugins.
 * A subcommand missing what it acts on opens the list it would pick from.
 */
export function pluginRequest(arg: string): PluginRequest {
  const [verb = "", ...rest] = arg.trim().split(/\s+/).filter(Boolean);
  const target = rest.join(" ");
  switch (verb.toLowerCase()) {
    case "":
    case "discover":
    case "browse":
      return { open: "discover", query: target };
    case "install":
    case "i":
      return rest.length ? { plugin: "install", id: rest[0] } : { open: "discover", query: "" };
    case "installed":
    case "list":
    case "manage":
      return { open: "installed", query: target };
    case "uninstall":
    case "remove":
    case "enable":
    case "disable":
    case "update": {
      const verbs = { uninstall: "uninstall", remove: "uninstall", enable: "enable", disable: "disable", update: "update" } as const;
      const plugin = verbs[verb.toLowerCase() as keyof typeof verbs];
      return rest.length ? { plugin, id: rest[0] } : { open: "installed", query: "" };
    }
    case "marketplace":
    case "marketplaces":
    case "market": {
      const [sub = "", ...args] = rest;
      const what = args.join(" ");
      switch (sub.toLowerCase()) {
        case "add":
          return what ? { marketplace: "add", source: what } : { open: "marketplaces", query: "" };
        case "remove":
        case "rm":
          return what ? { marketplace: "remove", name: what } : { open: "marketplaces", query: "" };
        case "update":
          return { marketplace: "update", name: what || null };
        default:
          return { open: "marketplaces", query: "" };
      }
    }
    default:
      return { open: "discover", query: arg.trim() };
  }
}

/** What a change is doing while it runs, e.g. "Installing tdd…"; null for opening the window. */
export function progress(request: PluginRequest): string | null {
  if ("open" in request) return null;
  if ("marketplace" in request) {
    if (request.marketplace === "add") return `Adding the marketplace ${request.source}…`;
    if (request.marketplace === "remove") return `Removing the marketplace ${request.name}…`;
    return request.name ? `Updating the marketplace ${request.name}…` : "Updating every marketplace…";
  }
  return `${DOING[request.plugin]} ${splitId(request.id).name}…`;
}
