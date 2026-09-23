/**
 * The second line of an agent's row in the sidebar: what it is doing now, or
 * how it ended. Pure: an agent and its events in, a few words out.
 */

import type { Agent, AgentEvent } from "$lib/api";
import { callTarget } from "$lib/transcript/toolCalls";

/** A tool call as the present-tense thing the agent is doing. */
function doing(name: string, input: unknown): string {
  const target = callTarget(name, input);
  switch (name) {
    case "Read":
      return `Reading ${target}`;
    case "Edit":
    case "Write":
    case "NotebookEdit":
      return `Editing ${target}`;
    // A Bash call's description already reads as an action: "Run the tests".
    case "Bash":
      return target || "Running a command";
    case "Grep":
    case "Glob":
      return `Searching for ${target}`;
    case "Task":
      return `Delegating: ${target}`;
    case "WebSearch":
      return `Searching the web for ${target}`;
    case "WebFetch":
      return `Reading ${target}`;
    case "TodoWrite":
      return "Updating its plan";
  }
  return target ? `${name}: ${target}` : name;
}

/**
 * What a running agent is doing, from the last thing it said or called in its
 * log. Empty until something has come in — an agent that was already working
 * when the app launched has no live events yet.
 */
export function latestActivity(events: AgentEvent[]): string {
  for (let i = events.length - 1; i >= 0; i--) {
    const e = events[i].event as { type?: string; message?: { content?: unknown } } | null;
    if (e?.type !== "assistant" || !Array.isArray(e.message?.content)) continue;
    const blocks = e.message.content as { type?: string; name?: string; input?: unknown }[];
    const last = blocks.at(-1);
    if (last?.type === "tool_use") return doing(last.name ?? "", last.input);
    if (last?.type === "thinking") return "Thinking…";
    if (last?.type === "text") return "Writing…";
  }
  return "";
}

/**
 * The row's second line. `delivered` and `holdingWork` come from the store,
 * which reads them from git rather than the record; `modelName` is the
 * agent's model as the picker names it, if it has one.
 */
export function rowDetail(
  a: Agent,
  events: AgentEvent[],
  { delivered, holdingWork, modelName }: { delivered: boolean; holdingWork: boolean; modelName: string },
): string {
  if (a.state === "running") {
    const now = latestActivity(events);
    if (now) return now;
    return a.resolves ? `Resolving a merge into ${a.resolves.target}` : "Working…";
  }
  if (delivered) return `Merged into ${a.merged_branch ?? "the project"}`;
  if (a.state === "failed")
    return a.fail_reason?.trim() || (a.exit_code != null ? `Exited with code ${a.exit_code}` : "Failed");
  if (a.state === "orphaned") return "Interrupted when the app closed";
  if (a.merged_at && holdingWork) return `New work since merging into ${a.merged_branch}`;
  const parts = [`${a.turns} repl${a.turns === 1 ? "y" : "ies"}`];
  if (modelName) parts.push(modelName);
  return parts.join(" · ");
}
