/**
 * Spend by calendar day, in the user's time zone. The backend buckets each
 * Agent's spend into quarter-hour slots because only this side knows where
 * the user's midnight falls; here those slots are folded into days.
 */

import type { AgentUsage } from "$lib/api";

export type DayUsage = {
  /** `YYYY-MM-DD` in local time — stable across refreshes, so a row key. */
  key: string;
  /** Local midnight opening the day. */
  start: Date;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_creation_tokens: number;
  cost_usd: number;
  turns: number;
};

/** How many days the list covers, today included. */
export const DAYS_SHOWN = 7;

/**
 * Add up every Agent's slots over the last week, newest day first. Every day
 * gets a row, spent on or not, so the week reads as a week.
 */
export function byDay(agents: AgentUsage[], now: Date): DayUsage[] {
  const days = new Map<string, DayUsage>();
  for (let i = 0; i < DAYS_SHOWN; i++) {
    const start = new Date(now.getFullYear(), now.getMonth(), now.getDate() - i);
    days.set(dayKey(start), {
      key: dayKey(start),
      start,
      input_tokens: 0,
      output_tokens: 0,
      cache_read_tokens: 0,
      cache_creation_tokens: 0,
      cost_usd: 0,
      turns: 0,
    });
  }
  for (const a of agents) {
    for (const s of a.slots) {
      const day = days.get(dayKey(new Date(s.start * 1000)));
      if (!day) continue;
      day.input_tokens += s.input_tokens;
      day.output_tokens += s.output_tokens;
      day.cache_read_tokens += s.cache_read_tokens;
      day.cache_creation_tokens += s.cache_creation_tokens;
      day.cost_usd += s.cost_usd;
      day.turns += s.turns;
    }
  }
  return [...days.values()];
}

/** "Today", "Yesterday", then "Mon, Sep 21" — with the year once it isn't this one. */
export function dayLabel(day: Date, now: Date): string {
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const yesterday = new Date(today.getFullYear(), today.getMonth(), today.getDate() - 1);
  if (dayKey(day) === dayKey(today)) return "Today";
  if (dayKey(day) === dayKey(yesterday)) return "Yesterday";
  return day.toLocaleDateString([], {
    weekday: "short",
    month: "short",
    day: "numeric",
    ...(day.getFullYear() === now.getFullYear() ? {} : { year: "numeric" }),
  });
}

function dayKey(d: Date): string {
  const mm = String(d.getMonth() + 1).padStart(2, "0");
  const dd = String(d.getDate()).padStart(2, "0");
  return `${d.getFullYear()}-${mm}-${dd}`;
}
