/** Run with `npm test`. */
import { test } from "node:test";
import assert from "node:assert/strict";
import type { AgentEvent } from "$lib/api";
import { buildRows, buildSubagentRows, isRunning, subagentReport, type Row } from "./rows.ts";

const ev = (event: object): AgentEvent => ({ ts: "2026-10-06T12:00:00Z", event }) as AgentEvent;
const say = (content: object[], within?: string, type = "assistant") =>
  ev({ type, message: { content }, parent_tool_use_id: within ?? null });
const use = (id: string, name: string, input: object = {}, within?: string) =>
  say([{ type: "tool_use", id, name, input }], within);
const answer = (id: string, text: string, within?: string) =>
  say([{ type: "tool_result", tool_use_id: id, content: [{ type: "text", text }] }], within, "user");
const task = (subtype: string, id: string, more: object = {}) =>
  ev({ type: "system", subtype, tool_use_id: id, ...more });

const shape = (rows: Row[]) =>
  rows.map((r) =>
    r.kind === "tools"
      ? `${r.name}×${r.calls.length}`
      : r.kind === "text" || r.kind === "prompt"
        ? `${r.kind}: ${r.text}`
        : r.kind,
  );

/** The Agent sends two Sub-agents off at once, and their work interleaves. */
const log = [
  ev({ type: "cw_prompt", prompt: "Look into it", turn: 1 }),
  use("a1", "Agent", { description: "Find the bug", prompt: "Find it", subagent_type: "Explore" }),
  task("task_started", "a1", { task_type: "local_agent", is_backgrounded: false }),
  say([{ type: "text", text: "Find it" }], "a1", "user"),
  use("a2", "Agent", { description: "Read the docs", prompt: "Read them" }),
  say([{ type: "text", text: "Read them" }], "a2", "user"),
  use("r1", "Read", { file_path: "/x/rows.ts" }, "a1"),
  answer("r1", "1\tcode", "a1"),
  task("task_progress", "a1"),
  use("g1", "Grep", { pattern: "bug" }, "a1"),
  use("r2", "Read", { file_path: "/x/README.md" }, "a2"),
  ev({ type: "tool_progress", parent_tool_use_id: "a1", elapsed_time_seconds: 30 }),
  answer("g1", "rows.ts:3", "a1"),
  task("task_notification", "a1", { status: "completed", usage: { duration_ms: 4000 } }),
  answer(
    "a1",
    [
      "[Subagent hand-back] The text below is the final report. The report follows:",
      "  It's on **line 3**.",
      "    indented",
      "agentId: abc (use SendMessage with to: 'abc')",
      "<usage>tool_uses: 2</usage>",
    ].join("\n"),
  ),
];

test("a Sub-agent's work stays out of the Agent's own transcript", () => {
  const rows = buildRows(log);
  assert.deepEqual(shape(rows), ["prompt: Look into it", "Agent×2"]);
});

test("each Sub-agent's call says how far it has got", () => {
  const rows = buildRows(log);
  const [a1, a2] = (rows.at(-1) as Row & { kind: "tools" }).calls;
  assert.equal(a1.id, "a1");
  assert.deepEqual([a1.steps, a1.lastStep, a1.elapsedSeconds, isRunning(a1)], [2, "Grep", 30, false]);
  assert.deepEqual([a2.steps, a2.lastStep, isRunning(a2)], [1, "Read", true]);
  assert.deepEqual(a2.lastStepInput, { file_path: "/x/README.md" });
});

test("a Sub-agent's own transcript runs from its prompt to its report", () => {
  const { call, rows } = buildSubagentRows(log, "a1");
  assert.equal(call?.id, "a1");
  assert.deepEqual(shape(rows), [
    "prompt: Find it",
    "Read×1",
    "Grep×1",
    "text: It's on **line 3**.\n  indented",
    "result",
  ]);
  assert.equal((rows.at(-1) as Row & { kind: "result" }).duration_ms, 4000);
});

test("a Sub-agent still at work has no report yet", () => {
  const { call, rows } = buildSubagentRows(log, "a2");
  assert.ok(call && isRunning(call));
  assert.deepEqual(shape(rows), ["prompt: Read them", "Read×1"]);
});

test("a Sub-agent whose prompt never came down the stream takes it from its call", () => {
  const evs = [use("t1", "Task", { prompt: "Do it" }), use("b", "Bash", {}, "t1")];
  const { rows } = buildSubagentRows(evs, "t1");
  assert.deepEqual(shape(rows), ["prompt: Do it", "Bash×1"]);
});

test("a Sub-agent sent to the background is at work until it reports back", () => {
  const started = [
    use("a1", "Agent", { prompt: "Go" }),
    task("task_started", "a1", { is_backgrounded: true }),
    answer("a1", "Launched in the background"),
  ];
  const away = buildSubagentRows(started, "a1");
  assert.ok(away.call && isRunning(away.call));
  assert.deepEqual(shape(away.rows), ["prompt: Go"]);
  const back = buildSubagentRows(
    [...started, task("task_notification", "a1", { status: "completed" })],
    "a1",
  );
  assert.ok(back.call && !isRunning(back.call));
});

test("a Sub-agent that isn't in the log has no transcript", () => {
  assert.deepEqual(buildSubagentRows(log, "gone"), { call: undefined, rows: [] });
});

test("a background command's start still makes a row", () => {
  const rows = buildRows([
    use("b1", "Bash", { command: "npm run dev", run_in_background: true }),
    task("task_started", "b1", { task_type: "local_bash", is_backgrounded: true }),
  ]);
  assert.deepEqual(shape(rows), ["Bash×1", "system"]);
});

test("a Sub-agent's report is shown as written when its frame isn't recognised", () => {
  assert.equal(subagentReport("  Just the answer.\n"), "Just the answer.");
});
