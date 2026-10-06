/**
 * Reading a tool call: what it was about, what it put in a file, what came
 * back, and how long it ran — the words and code the transcript shows for it.
 */

import { formatDuration } from "$lib/format";
import { languageOf } from "$lib/code/highlight.svelte";
import { outputLanguage, terminalText } from "./shell";
import { isRunning, type ToolCall } from "./rows";

function basename(path: string): string {
  return path.split("/").filter(Boolean).pop() ?? path;
}

export function truncate(s: string, n: number): string {
  const flat = s.replace(/\s+/g, " ").trim();
  return flat.length > n ? flat.slice(0, n - 1) + "…" : flat;
}

/**
 * What a call was *about*, in a few words: the file read, the pattern
 * searched for, the command run. It's the difference between a row that
 * says `Read ×8` and one that says which eight.
 */
export function callTarget(name: string, input: unknown): string {
  const o = (input && typeof input === "object" ? input : {}) as Record<string, any>;
  const str = (v: unknown) => (typeof v === "string" && v.trim() ? v.trim() : "");
  switch (name) {
    case "Read":
    case "Write":
    case "Edit":
      return basename(str(o.file_path));
    case "NotebookEdit":
      return basename(str(o.notebook_path));
    case "Bash":
      return str(o.description) || str(o.command);
    case "Grep":
    case "Glob":
      return str(o.pattern);
    case "Agent":
    case "Task":
      return str(o.description);
    case "Skill":
      return str(o.skill);
    case "WebSearch":
      return str(o.query);
    case "WebFetch":
      try {
        return new URL(str(o.url)).host;
      } catch {
        return str(o.url);
      }
    case "TodoWrite":
      return Array.isArray(o.todos) ? `${o.todos.length} items` : "";
  }
  // An unknown tool's first string argument is usually its subject.
  for (const v of Object.values(o)) {
    const s = str(v);
    if (s) return s;
  }
  return "";
}

/**
 * The targets of a grouped run. While a call in it is still going, it's
 * the only one named — each new call overwrites the last, so the row reads
 * as what's happening now. Once the run settles: the first couple, then a
 * count.
 */
export function groupTargets(calls: ToolCall[]): string {
  const running = calls.findLast((c) => !c.hasResult);
  if (running && calls.length > 1) return truncate(callTarget(running.name, running.input), 80);
  const seen: string[] = [];
  for (const c of calls) {
    const t = truncate(callTarget(c.name, c.input), 40);
    if (t && !seen.includes(t)) seen.push(t);
  }
  if (seen.length === 0) return "";
  const shown = seen.slice(0, 2);
  const rest = seen.length - shown.length;
  return rest > 0 ? `${shown.join(", ")}, +${rest}` : shown.join(", ");
}

/**
 * How long a slow call ran, from its heartbeats. They only come every 30s,
 * so a finished call ran at least this long, not exactly it.
 */
export function elapsedLabel(c: ToolCall): string {
  if (!c.elapsedSeconds) return "";
  const d = formatDuration(c.elapsedSeconds * 1000);
  return c.hasResult ? `ran over ${d}` : `running ${d}`;
}

/** The longest-running call in a group, for the group's own summary. */
export function groupElapsed(calls: ToolCall[]): string {
  const running = calls.filter((c) => !c.hasResult);
  const pool = running.length ? running : calls;
  const slowest = pool.reduce((a, b) => (b.elapsedSeconds > a.elapsedSeconds ? b : a));
  return elapsedLabel(slowest);
}

/** The kind of Sub-agent a call started: `Explore`, `Plan`, or the general one. */
export function subagentType(c: ToolCall): string {
  const t = (c.input as { subagent_type?: unknown } | null)?.subagent_type;
  return typeof t === "string" && t.trim() ? t.trim() : "general-purpose";
}

/**
 * How far a Sub-agent has got: how many tools it has called and, while it's
 * still at work, what it's doing now.
 */
export function subagentProgress(c: ToolCall): string {
  const n = c.steps ?? 0;
  const running = isRunning(c);
  const count = n
    ? `${n} tool ${n === 1 ? "call" : "calls"}`
    : running
      ? "Starting"
      : "No tool calls";
  if (!running || !c.lastStep) return count;
  const target = truncate(callTarget(c.lastStep, c.lastStepInput), 60);
  return `${count} · ${c.lastStep}${target ? ` ${target}` : ""}`;
}

/** A Bash call's command, if that's what `c` is. */
export function bashCommand(c: ToolCall): string | undefined {
  const o = c.input as Record<string, unknown> | null;
  return c.name === "Bash" && typeof o?.command === "string" ? o.command : undefined;
}

/**
 * The language a Bash call's output is in, if all it printed was a file or
 * a diff — not when it failed, since then it printed an error instead.
 */
export function bashOutputLanguage(c: ToolCall): string {
  const command = bashCommand(c);
  return command === undefined || c.isError ? "" : outputLanguage(command);
}

/**
 * What `fileChange` and `readLines` last gave for a call, by its input: the
 * one object that stays the same while the transcript rebuilds the call
 * around it on every event. Handing back the same answer is what keeps a
 * card's code from being highlighted afresh each time its Row changes —
 * which a result arriving does, and a highlighted file can be thousands of
 * lines.
 */
const changes = new WeakMap<object, ReturnType<typeof readFileChange>>();
const reads = new WeakMap<object, { result: unknown; lines: ReturnType<typeof parseRead> }>();

/**
 * What an Edit or Write call puts in a file, if that's what `c` is: an
 * Edit's text before and after, a Write's whole new contents.
 */
export function fileChange(c: ToolCall): ReturnType<typeof readFileChange> {
  const key = c.input;
  if (!key || typeof key !== "object") return readFileChange(c);
  if (!changes.has(key)) changes.set(key, readFileChange(c));
  return changes.get(key);
}

function readFileChange(
  c: ToolCall,
): { lang: string; before?: string; after: string; everywhere: boolean } | undefined {
  const o = c.input as Record<string, unknown> | null;
  const lang = languageOf(typeof o?.file_path === "string" ? o.file_path : "");
  if (c.name === "Edit" && typeof o?.old_string === "string" && typeof o?.new_string === "string")
    return { lang, before: o.old_string, after: o.new_string, everywhere: o.replace_all === true };
  if (c.name === "Write" && typeof o?.content === "string")
    return { lang, after: o.content, everywhere: false };
  return undefined;
}

/**
 * A Read call's result as the file's lines, split from the line numbers
 * Claude Code puts in front of each (`12→` in older versions, `12<tab>`
 * now), plus whatever trails them. A result that doesn't start that way —
 * an error, an image — isn't one of these.
 */
export function readLines(c: ToolCall): ReturnType<typeof parseRead> {
  const key = c.input;
  if (!key || typeof key !== "object") return parseRead(c);
  const last = reads.get(key);
  if (last && last.result === c.result) return last.lines;
  const lines = parseRead(c);
  reads.set(key, { result: c.result, lines });
  return lines;
}

function parseRead(
  c: ToolCall,
): { lang: string; lines: { n: string; text: string }[]; rest: string } | undefined {
  const o = c.input as Record<string, unknown> | null;
  if (c.name !== "Read" || typeof o?.file_path !== "string") return undefined;
  const all = toolResultText(c.result).split("\n");
  const lines: { n: string; text: string }[] = [];
  for (const line of all) {
    const m = /^\s*(\d+)(?:\t|→)(.*)$/.exec(line);
    if (!m) break;
    lines.push({ n: m[1], text: m[2] });
  }
  if (lines.length === 0) return undefined;
  return { lang: languageOf(o.file_path), lines, rest: all.slice(lines.length).join("\n").trim() };
}

/** What a call returned, as text; a command's as its terminal would show it. */
export function resultText(c: ToolCall): string {
  const text = toolResultText(c.result);
  return c.name === "Bash" ? terminalText(text) : text;
}

function toolResultText(content: unknown): string {
  if (typeof content === "string") return content;
  if (Array.isArray(content)) {
    return content
      .map((c: any) =>
        c?.type === "text"
          ? c.text
          : // Its bytes aren't kept (see `storage::slim`), so name it instead.
            c?.source?.type === "base64"
            ? `[${c.type}${c.source.media_type ? `: ${c.source.media_type}` : ""}]`
            : JSON.stringify(c),
      )
      .join("\n");
  }
  return JSON.stringify(content);
}
