/**
 * A Lead and its Helpers (ADR 0019), as the sidebar lists them: each Helper
 * tucked under its Lead, so the team reads as the one piece of work it is.
 * Pure: agents in, teams out.
 */

import type { Agent } from "$lib/api";

/** An agent and the Helpers it Spawned, oldest first. Most agents have none. */
export type Team = { lead: Agent; helpers: Agent[] };

/**
 * `agents` as teams, keeping their order. A Helper goes under its Lead, in
 * the order the Lead Spawned them. One whose Lead isn't in `agents`, because
 * it was Discarded, stands on its own. A Lead's id is its Host's, so it is
 * matched on the same Host.
 */
export function teams(agents: Agent[]): Team[] {
  const key = (host: string, id: string) => `${host}\n${id}`;
  const listed = new Set(agents.map((a) => key(a.host, a.id)));
  const helpers = new Map<string, Agent[]>();
  const tucked = new Set<Agent>();
  for (const a of agents) {
    if (!a.lead_id || !listed.has(key(a.host, a.lead_id))) continue;
    const lead = key(a.host, a.lead_id);
    helpers.set(lead, [...(helpers.get(lead) ?? []), a]);
    tucked.add(a);
  }
  return agents
    .filter((a) => !tucked.has(a))
    .map((lead) => ({
      lead,
      helpers: (helpers.get(key(lead.host, lead.id)) ?? []).toSorted(
        (x, y) => Date.parse(x.spawned_at) - Date.parse(y.spawned_at),
      ),
    }));
}

/** Every agent in the team, the Lead first. */
export function members(team: Team): Agent[] {
  return [team.lead, ...team.helpers];
}

/** Whether anyone in the team is working, which puts it all under Running. */
export function working(team: Team): boolean {
  return members(team).some((a) => a.state === "running");
}
