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
