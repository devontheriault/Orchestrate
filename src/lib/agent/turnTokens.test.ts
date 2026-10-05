/** Run with `npm test`. */
import { test } from "node:test";
import assert from "node:assert/strict";
import type { AgentEvent } from "$lib/api";
import { turnOutputTokens } from "./turnTokens.ts";

const at = (event: unknown): AgentEvent => ({ ts: "2026-10-05T12:00:00Z", event }) as AgentEvent;
const prompt = () => at({ type: "cw_prompt", prompt: "go", turn: 1 });
const said = (text: string) => at({ type: "assistant", message: { content: [{ type: "text", text }] } });
const wrote = (content: string) =>
  at({ type: "assistant", message: { content: [{ type: "tool_use", name: "Write", input: { content } }] } });
const thought = (n: number) => at({ type: "system", subtype: "thinking_tokens", estimated_tokens_delta: n });

test("counts what the running Turn produced, and nothing before its prompt", () => {
  const events = [prompt(), said("x".repeat(400)), prompt(), said("abcd"), wrote("hi"), thought(7)];
  // "abcd" is 4 characters and the Write's input `{"content":"hi"}` is 16.
  assert.equal(turnOutputTokens(events), 7 + Math.round(20 / 2));
});

test("a message already counted isn't serialized again as the Turn grows", () => {
  const events = [prompt(), ...Array.from({ length: 30 }, () => wrote("y".repeat(10_000)))];
  const total = turnOutputTokens(events);

  const stringify = JSON.stringify;
  let calls = 0;
  JSON.stringify = ((...args: Parameters<typeof stringify>) => {
    calls++;
    return stringify(...args);
  }) as typeof stringify;
  try {
    events.push(wrote("z"));
    const grown = turnOutputTokens(events);
    assert.equal(calls, 1);
    assert.equal(grown, total + Math.round(stringify({ content: "z" }).length / 2));
  } finally {
    JSON.stringify = stringify;
  }
});
