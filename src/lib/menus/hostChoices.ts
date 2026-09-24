import type { Agent } from "$lib/api";
import { canSpawnOn, checkoutOn, type ProjectGroup } from "$lib/state/projects";

export type HostChoice = { id: string; name: string; note: string; disabled: boolean };

/**
 * The Hosts a new agent in `group` could start on, as the Host picker lists
 * them: every Host the window knows, each saying whether it has the project,
 * will clone it, or can't be used right now and why.
 */
export function hostChoices(
  group: ProjectGroup,
  ids: string[],
  label: (id: string) => string,
  problem: (id: string) => string | null,
): HostChoice[] {
  return ids.map((id) => {
    const trouble = problem(id);
    const spawnable = canSpawnOn(group, id);
    const note =
      trouble ??
      (checkoutOn(group, id)
        ? "Has this project"
        : spawnable
          ? "Clones it from the remote first"
          : "Doesn't have it, and it has no remote to clone");
    return { id, name: label(id), note, disabled: !!trouble || !spawnable };
  });
}

/**
 * Where the next prompt to `agent` could run: on its own Host, as a reply, or
 * on another, as a new agent that picks up its committed work (ADR 0013).
 * Null when there's nowhere else it could go — one Host, or a project with no
 * remote for the work to travel through.
 */
export function handoffChoices(
  agent: Pick<Agent, "host">,
  group: ProjectGroup,
  ids: string[],
  label: (id: string) => string,
  problem: (id: string) => string | null,
): HostChoice[] | null {
  if (!group.remote || !ids.some((id) => id !== agent.host)) return null;
  return hostChoices(group, ids, label, problem).map((c) => {
    if (c.id === agent.host) return { ...c, note: "Replies to this agent" };
    if (c.disabled) return c;
    const note = checkoutOn(group, c.id)
      ? "A new agent there, from this one's commits"
      : "Clones the project, then a new agent from this one's commits";
    return { ...c, note };
  });
}
