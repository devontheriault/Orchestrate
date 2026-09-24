import type { Machine } from "$lib/api";

/**
 * The machines the Hosts dialog offers under its name box: the user's tailnet
 * machines this window doesn't have yet, narrowed by what's typed.
 */
export function machineChoices(
  machines: Machine[],
  added: string[],
  typed: string,
): Machine[] {
  const q = typed.trim().toLowerCase();
  return machines.filter((m) => !added.includes(m.name) && m.name.includes(q));
}

/** A machine's line under its name: whether it's up, and what it runs. */
export function machineNote(m: Machine): string {
  return `${m.online ? "Online" : "Offline"} · ${m.os}`;
}
