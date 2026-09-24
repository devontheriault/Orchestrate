/**
 * One project across every Host (ADR 0011). A repository with a remote is one
 * project wherever it's checked out, known by its remote; one with no remote
 * is only ever on the Host that has it. The window groups the checkouts each
 * Host lists into these, and everything the user sees works on the groups.
 */

import type { Project } from "$lib/api";

export type ProjectGroup = {
  /** The remote, normalized, or `host/id` for a project with no remote. */
  id: string;
  name: string;
  /** Where it is, for display: this machine's checkout, else `host: path`. */
  path: string;
  /** The remote as git has it, for cloning onto a Host that has no checkout. */
  remote: string | null;
  /** Every Host's checkout, this machine's first. */
  checkouts: Project[];
  added_at: string;
};

/**
 * A remote URL reduced to what names the repository, so `git@github.com:a/b.git`
 * and `https://github.com/a/b` are the same project: host and path, no user,
 * scheme, port or `.git`, lower-cased as the big hosts treat them.
 */
export function normalizeRemote(url: string): string {
  let u = url.trim().replace(/\/+$/, "").replace(/\.git$/, "");
  const scheme = u.match(/^[a-z][a-z0-9+.-]*:\/\/(.*)$/i);
  if (scheme) {
    if (/^file:\/\//i.test(u)) return u.slice("file://".length);
    // user@host:port/path → host/path
    u = scheme[1].replace(/^[^@/]*@/, "").replace(/^([^/:]+):\d+/, "$1");
  } else if (!u.startsWith("/")) {
    // scp-like: user@host:path
    const scp = u.match(/^(?:[^@]+@)?([^:/]+):(.*)$/);
    if (scp) u = `${scp[1]}/${scp[2].replace(/^\/+/, "")}`;
  }
  return u.toLowerCase();
}

/** Which project a checkout belongs to. */
export function groupKey(p: Project): string {
  return p.remote ? normalizeRemote(p.remote) : `${p.host}/${p.id}`;
}

/**
 * Every Host's checkouts, grouped into projects in the order they first
 * appear — this machine's first, so a single-machine window looks as it
 * always has.
 */
export function groupProjects(projects: Project[], local: string): ProjectGroup[] {
  const ordered = [
    ...projects.filter((p) => p.host === local),
    ...projects.filter((p) => p.host !== local),
  ];
  const groups = new Map<string, ProjectGroup>();
  for (const p of ordered) {
    const id = groupKey(p);
    const group = groups.get(id);
    if (group) {
      group.checkouts.push(p);
      // A name the user gave beats one a Host cloned under.
      if (group.checkouts.every((c) => c === p || c.cloned) && !p.cloned) group.name = p.name;
      continue;
    }
    groups.set(id, {
      id,
      name: p.name,
      path: p.host === local ? p.path : `${p.host}: ${p.path}`,
      remote: p.remote,
      checkouts: [p],
      added_at: p.added_at,
    });
  }
  return [...groups.values()];
}

/**
 * The checkout new agents on `host` use: one the user registered there wins
 * over one the Host cloned for itself.
 */
export function checkoutOn(group: ProjectGroup, host: string): Project | undefined {
  const here = group.checkouts.filter((c) => c.host === host);
  return here.find((c) => !c.cloned) ?? here[0];
}

/** Whether an agent in `group` can be spawned on `host`: it has a checkout, or can clone one. */
export function canSpawnOn(group: ProjectGroup, host: string): boolean {
  return !!group.remote || group.checkouts.some((c) => c.host === host);
}

/**
 * Where a new agent in `group` starts unless the user picks otherwise: the Host
 * it last started on, while that one can be reached; else this machine, where
 * the project is here or can be cloned; else the Host that has it.
 */
export function defaultHost(
  group: ProjectGroup,
  last: string | undefined,
  reachable: (host: string) => boolean,
  local: string,
): string {
  if (last && canSpawnOn(group, last) && reachable(last)) return last;
  if (canSpawnOn(group, local)) return local;
  return group.checkouts[0]?.host ?? local;
}

/** The Hosts a project has a checkout on. */
export function hostsOf(group: ProjectGroup): string[] {
  return [...new Set(group.checkouts.map((c) => c.host))];
}
