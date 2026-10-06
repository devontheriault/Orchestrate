/** Run with `npm test`. */
import { test } from "node:test";
import assert from "node:assert/strict";
import type { Agent } from "$lib/api";
import { teams, working } from "./teams.ts";

const agent = (id: string, spawned: number, more: Partial<Agent> = {}): Agent =>
  ({
    id,
    host: "local",
    state: "completed",
    spawned_at: new Date(Date.UTC(2026, 9, 6, 12, spawned)).toISOString(),
    ...more,
  }) as Agent;

const ids = (list: { lead: Agent; helpers: Agent[] }[]) =>
  list.map((t) => [t.lead.id, t.helpers.map((h) => h.id)]);

test("Helpers go under their Lead, in the order it Spawned them", () => {
  // Newest first, as the store lists them.
  const listed = [
    agent("h2", 4, { lead_id: "lead" }),
    agent("other", 3),
    agent("h1", 2, { lead_id: "lead" }),
    agent("lead", 1),
  ];
  assert.deepEqual(ids(teams(listed)), [
    ["other", []],
    ["lead", ["h1", "h2"]],
  ]);
});

test("a Helper whose Lead is gone stands on its own", () => {
  assert.deepEqual(ids(teams([agent("h1", 2, { lead_id: "discarded" })])), [["h1", []]]);
});

test("a Lead is matched on its Helper's own Host", () => {
  const listed = [agent("h1", 2, { lead_id: "lead", host: "desk" }), agent("lead", 1)];
  assert.deepEqual(ids(teams(listed)), [
    ["h1", []],
    ["lead", []],
  ]);
});

test("a team is working while any of it is", () => {
  const [team] = teams([agent("h1", 2, { lead_id: "lead", state: "running" }), agent("lead", 1)]);
  assert.equal(working(team), true);
  const [idle] = teams([agent("lead", 1)]);
  assert.equal(working(idle), false);
});
