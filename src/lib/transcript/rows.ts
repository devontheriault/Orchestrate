/**
 * An Agent's log, as the Rows the transcript renders — see Row and Snapshot
 * tool in CONTEXT.md. Pure: events in, Rows out.
 */

import type { AgentEvent } from "$lib/api";

// Helpers to decode common stream-json event shapes without breaking on
// unknowns. Anything unrecognised falls through to the raw-JSON card.
type Kind =
  | { kind: "prompt"; text: string; attachments: string[] }
  | { kind: "notice"; text: string }
  | { kind: "system"; subtype: string }
  | { kind: "text"; text: string }
  | { kind: "tool_use"; name: string; input: unknown; id: string }
  | { kind: "tool_result"; tool_use_id: string; content: unknown; isError: boolean }
  | { kind: "thinking"; text: string }
  | { kind: "progress"; tool_use_id: string; seconds: number }
  | {
      kind: "task";
      subtype: string;
      tool_use_id: string;
      background: boolean;
      ended: boolean;
      duration_ms?: number;
    }
  | {
      kind: "result";
      result: string;
      is_error: boolean;
      duration_ms?: number;
      output_tokens?: number;
    }
  | { kind: "raw"; type?: string };

/**
 * Pure telemetry that `claude` emits every turn (sometimes several times)
 * but carries nothing a user reading the transcript can act on. Dropped
 * rather than rendered, so the stream doesn't read as the same line
 * repeated. A Host of this build never sends the first two
 * (`storage::unseen`); one from an older build still does. The task ones
 * report on a Sub-agent, which its call's card already tells from the
 * Sub-agent's own events.
 */
const SILENT_EVENT_TYPES = new Set(["rate_limit_event"]);
const SILENT_SYSTEM_SUBTYPES = new Set(["thinking_tokens", "task_progress", "task_updated"]);

/**
 * Claude Code's own tool for handing part of the work to a Sub-agent: `Task`
 * in older versions, `Agent` now. Everything the Sub-agent does comes down
 * the same stream, each event naming the call that started it in
 * `parent_tool_use_id`.
 */
export const SUBAGENT_TOOLS = new Set(["Agent", "Task"]);

/**
 * Claude Code's system subtypes, in the words the user reads them in. The
 * set is open-ended — the CLI adds to it without telling us — so anything
 * unknown falls back to its own name as a sentence rather than leaking the
 * protocol's snake_case into the transcript.
 */
const SYSTEM_LABELS: Record<string, string> = {
  "": "Session update",
  init: "Session started",
  compact_boundary: "Conversation compacted to free up context",
  conversation_reset: "Context cleared — the conversation starts afresh from here",
};

/** `compact_boundary` -> `Compact boundary`. */
export function humanize(s: string): string {
  const words = s.replace(/[_-]+/g, " ").trim();
  return words ? words[0].toUpperCase() + words.slice(1) : "";
}

export function systemLabel(subtype: string): string {
  return SYSTEM_LABELS[subtype] ?? humanize(subtype);
}

/**
 * Each event's blocks, worked out once: the log is rebuilt into Rows on every
 * event that arrives, and an event never changes after it has. `within` is
 * the Sub-agent's call it belongs to, "" for the Agent's own, or `ANY` for a
 * heartbeat, which finds its call wherever it is.
 */
type Classified = { within: string; kinds: Kind[] };
const classified = new WeakMap<AgentEvent, Classified>();
const ANY = "*";

function classify(ev: AgentEvent): Classified {
  let c = classified.get(ev);
  if (!c) {
    const kinds = classifyEvent(ev);
    const e = ev.event as { type?: string; parent_tool_use_id?: string | null } | null;
    // A heartbeat's `parent_tool_use_id` is the call it times, not its Sub-agent.
    const within =
      e?.type === "tool_progress"
        ? ANY
        : typeof e?.parent_tool_use_id === "string"
          ? e.parent_tool_use_id
          : "";
    classified.set(ev, (c = { within, kinds }));
  }
  return c;
}

function classifyEvent(ev: AgentEvent): Kind[] {
  const e = ev.event as {
    type?: string;
    message?: any;
    subtype?: string;
    result?: string;
    is_error?: boolean;
    prompt?: string;
    attachments?: string[];
    text?: string;
    duration_ms?: number;
    parent_tool_use_id?: string;
    elapsed_time_seconds?: number;
    tool_use_id?: string;
    is_backgrounded?: boolean;
    usage?: { output_tokens?: number; duration_ms?: number };
  } | null;
  if (!e || typeof e !== "object") return [{ kind: "raw" }];
  if (SILENT_EVENT_TYPES.has(e.type ?? "")) return [];
  // Our own event, not claude's: the prompt the user sent for this turn.
  if (e.type === "cw_prompt") {
    return [{ kind: "prompt", text: e.prompt ?? "", attachments: e.attachments ?? [] }];
  }
  // Also ours: something the app did on the agent's behalf, like finishing
  // the merge a resolver was spawned for.
  if (e.type === "cw_notice") return [{ kind: "notice", text: e.text ?? "" }];
  // What `/clear` leaves behind: Claude Code moves on to a new session, and the
  // runtime follows it there, so the next Turn really does start empty.
  if (e.type === "conversation_reset") return [{ kind: "system", subtype: e.type }];
  if (e.type === "system") {
    if (SILENT_SYSTEM_SUBTYPES.has(e.subtype ?? "")) return [];
    // A Sub-agent's start and end, or a background command's: which tell a
    // Sub-agent sent to the background apart from one that's finished.
    if ((e.subtype === "task_started" || e.subtype === "task_notification") && e.tool_use_id) {
      return [
        {
          kind: "task",
          subtype: e.subtype,
          tool_use_id: e.tool_use_id,
          background: e.is_backgrounded === true,
          ended: e.subtype === "task_notification",
          duration_ms: e.usage?.duration_ms,
        },
      ];
    }
    return [{ kind: "system", subtype: e.subtype ?? "" }];
  }
  // A heartbeat `claude` sends every 30s while a tool is still going. It
  // belongs to the call it's timing, not the transcript: one row per
  // heartbeat reads as the same line repeated.
  if (e.type === "tool_progress") {
    return [
      {
        kind: "progress",
        tool_use_id: e.parent_tool_use_id ?? "",
        seconds: e.elapsed_time_seconds ?? 0,
      },
    ];
  }
  if (e.type === "result")
    return [
      {
        kind: "result",
        result: e.result ?? "",
        is_error: !!e.is_error,
        duration_ms: e.duration_ms,
        output_tokens: e.usage?.output_tokens,
      },
    ];
  if ((e.type === "assistant" || e.type === "user") && e.message?.content) {
    const blocks = Array.isArray(e.message.content) ? e.message.content : [];
    return blocks
      .map((b: any): Kind => {
        // A Sub-agent's prompt comes to it as a message from its "user".
        if (b.type === "text" && e.type === "user" && e.parent_tool_use_id)
          return { kind: "prompt", text: b.text ?? "", attachments: [] };
        if (b.type === "text") return { kind: "text", text: b.text ?? "" };
        if (b.type === "tool_use")
          return { kind: "tool_use", name: b.name ?? "?", input: b.input, id: b.id ?? "" };
        if (b.type === "tool_result")
          return {
            kind: "tool_result",
            tool_use_id: b.tool_use_id ?? "",
            content: b.content,
            isError: b.is_error === true,
          };
        if (b.type === "thinking") return { kind: "thinking", text: b.thinking ?? "" };
        return { kind: "raw", type: b.type };
      })
      // Redacted/interleaved thinking often carries a signature but no
      // visible text — an empty "thinking" card tells the user nothing.
      .filter((k: Kind) => !(k.kind === "thinking" && !k.text.trim()));
  }
  return [{ kind: "raw", type: e.type }];
}

/**
 * One tool call and the result that came back for it. Claude Code reports
 * them as two separate events a few messages apart; the transcript reads
 * better as one thing, so we pair them on `tool_use_id` and render the
 * result inside the call that asked for it.
 */
export type ToolCall = {
  key: string;
  /** Claude Code's id for the call; a Sub-agent's events carry it. */
  id: string;
  name: string;
  input: unknown;
  result: unknown;
  hasResult: boolean;
  isError: boolean;
  /** How long the call had been running at its latest heartbeat. */
  elapsedSeconds: number;
  /**
   * For a call that started a Sub-agent: how many tools it has called, and
   * the latest. Kept flat, so `keepUnchanged` can tell an unchanged call.
   */
  steps?: number;
  lastStep?: string;
  lastStepInput?: unknown;
  /**
   * A Sub-agent sent to the background that hasn't reported back: its call
   * has returned, but its work hasn't.
   */
  away?: boolean;
};

/** Whether a Sub-agent's call, or any other, is still going. */
export function isRunning(c: ToolCall): boolean {
  return !c.hasResult || c.away === true;
}

/**
 * A row of the transcript. Several stream events can collapse into one row:
 * a run of same-tool calls, or a stretch of thinking. Rows carry a stable
 * `key` taken from the first event they cover, so a row that grows as the
 * turn runs keeps the same DOM node — and with it whatever the user had
 * expanded.
 */
export type Row =
  | { key: string; kind: "prompt"; text: string; attachments: string[] }
  | { key: string; kind: "notice"; text: string }
  | { key: string; kind: "text"; text: string }
  | { key: string; kind: "thinking"; parts: string[] }
  | { key: string; kind: "tools"; name: string; calls: ToolCall[] }
  | { key: string; kind: "system"; subtype: string }
  | {
      key: string;
      kind: "result";
      result: string;
      is_error: boolean;
      duration_ms?: number;
      output_tokens?: number;
    }
  | { key: string; kind: "raw"; type?: string; event: unknown };

/**
 * Tools whose input is a snapshot of a whole state rather than an action:
 * the tenth todo list supersedes the nine before it, so only the last one
 * in a turn is worth a row. Scoped to the turn rather than the whole
 * transcript — reading back an old turn should show the list as it stood
 * when that turn ended, not today's.
 */
const SNAPSHOT_TOOLS = new Set(["TodoWrite"]);

/** The Agent's own transcript, with each Sub-agent's work left in its call. */
export function buildRows(evs: AgentEvent[]): Row[] {
  return build(evs, "").rows;
}

/**
 * One Sub-agent's transcript: the prompt it was given, what it did, and the
 * report it handed back. `call` is the call that started it, as the Agent's
 * own transcript has it; none means there's no such Sub-agent any more, as
 * after a `/clear`.
 */
export function buildSubagentRows(
  evs: AgentEvent[],
  id: string,
): { call: ToolCall | undefined; rows: Row[] } {
  return build(evs, id);
}

function build(evs: AgentEvent[], within: string): { call: ToolCall | undefined; rows: Row[] } {
  // Flatten every event into its blocks first, tagged with the turn they
  // fall in, so superseded snapshots can be spotted before anything is
  // grouped.
  let items: { key: string; k: Kind; turn: number; within: string; event: unknown }[] = [];
  let turn = 0;
  let clearedTurn = -1;
  for (let i = 0; i < evs.length; i++) {
    const c = classify(evs[i]);
    for (let j = 0; j < c.kinds.length; j++) {
      const k = c.kinds[j];
      if (k.kind === "prompt" && c.within === "") turn++;
      // `/clear` clears the pane too: everything before it, the `/clear`
      // itself included, belongs to a conversation that's gone. The new
      // session's `init` follows, and reads as "Session started" again.
      if (k.kind === "system" && k.subtype === "conversation_reset") {
        items = [];
        clearedTurn = turn;
      }
      // The clearing Turn's "done" says nothing the reset row didn't.
      if (k.kind === "result" && turn === clearedTurn) continue;
      items.push({ key: `${i}:${j}`, k, turn, within: c.within, event: evs[i].event });
    }
  }

  const liveSnapshot = new Map<string, string>();
  // What each Sub-agent has been up to, for the call that started it.
  type Sub = {
    steps: number;
    last?: Kind & { kind: "tool_use" };
    background: boolean;
    ended: boolean;
    ms?: number;
  };
  const subs = new Map<string, Sub>();
  const sub = (id: string) => {
    let s = subs.get(id);
    if (!s) subs.set(id, (s = { steps: 0, background: false, ended: false }));
    return s;
  };
  for (const it of items) {
    const k = it.k;
    if (k.kind === "tool_use" && SNAPSHOT_TOOLS.has(k.name) && it.within === within) {
      liveSnapshot.set(`${it.turn}:${k.name}`, it.key);
    }
    if (k.kind === "tool_use" && it.within !== "" && it.within !== ANY) {
      const s = sub(it.within);
      s.steps++;
      s.last = k;
    }
    if (k.kind === "task") {
      const s = sub(k.tool_use_id);
      s.background ||= k.background;
      if (k.ended) {
        s.ended = true;
        s.ms = k.duration_ms;
      }
    }
  }

  const out: Row[] = [];
  const callsById = new Map<string, ToolCall>();
  const seenSystem = new Set<string>();
  // The call that started the Sub-agent these Rows are for. It's the Agent's,
  // not the Sub-agent's, so it's found among the events left out.
  let self: ToolCall | undefined;

  for (const it of items) {
    const k = it.k;
    const last = out[out.length - 1];

    if (it.within !== within && it.within !== ANY) {
      if (k.kind === "tool_use" && within && k.id === within) self = newCall(it.key, k);
      else if (k.kind === "tool_result" && self && k.tool_use_id === within) settle(self, k);
      continue;
    }

    if (k.kind === "tool_use") {
      if (SNAPSHOT_TOOLS.has(k.name) && liveSnapshot.get(`${it.turn}:${k.name}`) !== it.key) {
        continue;
      }
      const call = newCall(it.key, k);
      if (k.id) callsById.set(k.id, call);
      if (last?.kind === "tools" && last.name === k.name) last.calls.push(call);
      else out.push({ key: it.key, kind: "tools", name: k.name, calls: [call] });
    } else if (k.kind === "tool_result") {
      // No call on record means it belonged to a superseded snapshot; the
      // result goes with it.
      const call = callsById.get(k.tool_use_id);
      if (call) settle(call, k);
    } else if (k.kind === "progress") {
      const call = callsById.get(k.tool_use_id) ?? (k.tool_use_id === within ? self : undefined);
      if (call) call.elapsedSeconds = Math.max(call.elapsedSeconds, k.seconds);
    } else if (k.kind === "task") {
      // Read into `subs` above. A Sub-agent's card says it started and
      // finished; a background command's start and end still make a row.
      if (SUBAGENT_TOOLS.has(callsById.get(k.tool_use_id)?.name ?? "")) continue;
      if (seenSystem.has(k.subtype)) continue;
      seenSystem.add(k.subtype);
      out.push({ key: it.key, kind: "system", subtype: k.subtype });
    } else if (k.kind === "thinking") {
      if (last?.kind === "thinking") last.parts.push(k.text);
      else out.push({ key: it.key, kind: "thinking", parts: [k.text] });
    } else if (k.kind === "system") {
      // `init` and friends repeat every turn and say the same thing each
      // time; the first one is the only one that informs.
      if (seenSystem.has(k.subtype)) continue;
      seenSystem.add(k.subtype);
      out.push({ key: it.key, kind: "system", subtype: k.subtype });
    } else if (k.kind === "raw") {
      out.push({ key: it.key, kind: "raw", type: k.type, event: it.event });
    } else {
      out.push({ key: it.key, ...k });
    }
  }

  const describe = (id: string, call: ToolCall) => {
    if (!SUBAGENT_TOOLS.has(call.name)) return;
    const s = subs.get(id);
    call.steps = s?.steps ?? 0;
    call.lastStep = s?.last?.name ?? "";
    call.lastStepInput = s?.last?.input;
    call.away = !!s?.background && !s.ended;
  };
  callsById.forEach((call, id) => describe(id, call));
  if (self) describe(within, self);

  if (!within) return { call: undefined, rows: out };
  if (!self) return { call: undefined, rows: [] };
  // Claude Code sends a Sub-agent's prompt down the stream as its first
  // event. Should one not, the call that started it has it too.
  if (out[0]?.kind !== "prompt") {
    const prompt = (self.input as { prompt?: unknown } | null)?.prompt;
    if (typeof prompt === "string") {
      out.unshift({ key: `${self.key} prompt`, kind: "prompt", text: prompt, attachments: [] });
    }
  }
  if (self.hasResult && !self.away) {
    const report = subagentReport(resultText(self.result));
    if (report) out.push({ key: `${self.key} report`, kind: "text", text: report });
    out.push({
      key: `${self.key} done`,
      kind: "result",
      result: "",
      is_error: self.isError,
      duration_ms: subs.get(within)?.ms,
    });
  }
  return { call: self, rows: out };
}

function newCall(key: string, k: Kind & { kind: "tool_use" }): ToolCall {
  return {
    key,
    id: k.id,
    name: k.name,
    input: k.input,
    result: undefined,
    hasResult: false,
    isError: false,
    elapsedSeconds: 0,
  };
}

function settle(call: ToolCall, k: Kind & { kind: "tool_result" }) {
  call.result = k.content;
  call.hasResult = true;
  call.isError = k.isError;
}

function resultText(content: unknown): string {
  if (typeof content === "string") return content;
  if (!Array.isArray(content)) return "";
  return content
    .map((c) => (c?.type === "text" && typeof c.text === "string" ? c.text : ""))
    .filter(Boolean)
    .join("\n");
}

/**
 * What a Sub-agent said in the end, out of the frame Claude Code hands it to
 * the Agent in: a preamble telling the Agent it's not the user talking, the
 * report indented under it, and a footer of the Sub-agent's id and usage.
 * The frame changes between versions, so whatever doesn't match is left be.
 */
export function subagentReport(text: string): string {
  const FOLLOWS = "The report follows:\n";
  const at = text.indexOf(FOLLOWS);
  let report = at >= 0 ? text.slice(at + FOLLOWS.length) : text;
  report = report.replace(/\n+agentId: [^\n]*(\n<usage>[\s\S]*<\/usage>)?\s*$/, "");
  if (at >= 0) report = report.replace(/^ {2}/gm, "");
  return report.trim();
}

/**
 * `next`, with every Row and call that came out the same as last time swapped
 * back for last time's object — and `prev` itself when nothing changed at
 * all. `buildRows` makes everything anew, and a new object reads as a change
 * to whatever renders it: without this, each event re-rendered every Row in
 * the transcript, Markdown and highlighted code included.
 */
export function keepUnchanged(prev: Row[], next: Row[]): Row[] {
  const before = new Map(prev.map((r) => [r.key, r]));
  let changed = prev.length !== next.length;
  const out = next.map((row, i) => {
    const old = before.get(row.key);
    let kept: Row = row;
    if (old && same(old, row)) kept = old;
    else if (old?.kind === "tools" && row.kind === "tools") {
      const calls = new Map(old.calls.map((c) => [c.key, c]));
      kept = {
        ...row,
        calls: row.calls.map((c) => {
          const was = calls.get(c.key);
          return was && same(was, c) ? was : c;
        }),
      };
    }
    if (kept !== prev[i]) changed = true;
    return kept;
  });
  return changed ? out : prev;
}

/**
 * Equal field by field, looking `depth` levels into arrays and objects.
 * Anything deeper is compared by identity, which is enough: it comes straight
 * from an event, and an event's objects are the same ones every time.
 */
function same(a: unknown, b: unknown, depth = 3): boolean {
  if (a === b) return true;
  if (depth === 0 || typeof a !== "object" || typeof b !== "object" || !a || !b) return false;
  if (Array.isArray(a) !== Array.isArray(b)) return false;
  const ka = Object.keys(a);
  if (ka.length !== Object.keys(b).length) return false;
  return ka.every((k) => same((a as any)[k], (b as any)[k], depth - 1));
}
