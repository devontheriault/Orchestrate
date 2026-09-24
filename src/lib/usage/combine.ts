import type { ModelUsage, SlotUsage, UsageSummary } from "$lib/api";

/**
 * Every Host's usage as one account's (see Usage in CONTEXT.md). Each Host
 * reads the Claude Code sessions on its own machine, so the account's spend
 * is their sum; the rate-limit windows belong to the account, not a machine,
 * so the most recently reported stand for all of them.
 */
export function combineUsage(summaries: UsageSummary[]): UsageSummary | null {
  if (summaries.length === 0) return null;
  if (summaries.length === 1) return summaries[0];
  return {
    account: {
      models: sumModels(summaries.flatMap((s) => s.account.models)),
      slots: sumSlots(summaries.flatMap((s) => s.account.slots)),
      turns: summaries.reduce((n, s) => n + s.account.turns, 0),
    },
    agents: summaries.flatMap((s) => s.agents),
    limits:
      summaries
        .map((s) => s.limits)
        .filter((l) => l !== null)
        .sort((a, b) => Date.parse(b.observed_at) - Date.parse(a.observed_at))[0] ?? null,
  };
}

function sumModels(models: ModelUsage[]): ModelUsage[] {
  const by = new Map<string, ModelUsage>();
  for (const m of models) {
    const into = by.get(m.model);
    if (!into) {
      by.set(m.model, { ...m });
      continue;
    }
    into.input_tokens += m.input_tokens;
    into.output_tokens += m.output_tokens;
    into.cache_read_tokens += m.cache_read_tokens;
    into.cache_creation_tokens += m.cache_creation_tokens;
    into.cost_usd += m.cost_usd;
    into.context_window ??= m.context_window;
  }
  return [...by.values()].sort((a, b) => b.cost_usd - a.cost_usd);
}

function sumSlots(slots: SlotUsage[]): SlotUsage[] {
  const by = new Map<number, SlotUsage>();
  for (const s of slots) {
    const into = by.get(s.start);
    if (!into) {
      by.set(s.start, { ...s });
      continue;
    }
    into.input_tokens += s.input_tokens;
    into.output_tokens += s.output_tokens;
    into.cache_read_tokens += s.cache_read_tokens;
    into.cache_creation_tokens += s.cache_creation_tokens;
    into.cost_usd += s.cost_usd;
    into.turns += s.turns;
  }
  return [...by.values()].sort((a, b) => a.start - b.start);
}
