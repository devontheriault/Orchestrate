import type { AgentEvent } from "$lib/api";

/**
 * Roughly how many characters of text or tool input make one output token.
 * Fitted against the exact totals in `result` events on real logs, where
 * this landed within about 10% of the non-thinking output; code and JSON
 * tokenize far denser than the usual 4-per-token rule of thumb for prose.
 */
const CHARS_PER_TOKEN = 2;

/**
 * An estimate of the tokens the running turn has generated so far, meant to
 * climb as the agent works the way Claude Code's own spinner count does.
 *
 * The `usage` on `assistant` events can't be summed for this: `claude` takes
 * it from the start of each API call, so its `output_tokens` is a single
 * digit no matter how long the message runs. Instead this counts what came
 * out: `thinking_tokens` events for the thinking, which arrives redacted, and
 * the length of the text and tool calls for the rest.
 */
export function turnOutputTokens(events: AgentEvent[]): number {
  let since = events.length;
  while (since > 0 && (events[since - 1].event as any)?.type !== "cw_prompt") since--;

  let thinking = 0;
  let chars = 0;
  for (let i = since; i < events.length; i++) {
    const e = events[i].event as any;
    if (e?.type === "system" && e.subtype === "thinking_tokens") {
      if (typeof e.estimated_tokens_delta === "number") thinking += e.estimated_tokens_delta;
    } else if (e?.type === "assistant" && Array.isArray(e.message?.content)) {
      chars += charsOf(e.message.content);
    }
  }
  return thinking + Math.round(chars / CHARS_PER_TOKEN);
}

/**
 * Each message's length, counted once. This runs on every event of the
 * Turn, and serializing every tool call's input again each time grew with
 * the Turn: a few dozen Writes is megabytes per event.
 */
const counted = new WeakMap<object, number>();

function charsOf(content: any[]): number {
  let chars = counted.get(content);
  if (chars !== undefined) return chars;
  chars = 0;
  for (const b of content) {
    if (b?.type === "text") chars += (b.text ?? "").length;
    else if (b?.type === "tool_use") chars += JSON.stringify(b.input ?? {}).length;
  }
  counted.set(content, chars);
  return chars;
}
