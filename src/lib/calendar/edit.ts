/**
 * The arithmetic of writing a Calendar event: a draft from a click, a drag
 * or an existing one, the repeat choices, the guests typed in, and where a
 * dragged Calendar event lands. Pure, so it's tested on its own
 * (`edit.test.ts`); `EventEditor.svelte` and the grids only draw.
 *
 * Times are the window's: local `Date`s, written as wall-clock times in the
 * window's own zone, which the draft names.
 */
import type { CalendarDraft, CalendarEvent } from "$lib/api";
import { addDays, bounds, dayKey, parseDay, startOfDay } from "./layout.ts";

const MINUTE = 60_000;

/** A local time as a draft writes it: `YYYY-MM-DDTHH:MM`. */
export function wallClock(d: Date): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `${dayKey(d)}T${p(d.getHours())}:${p(d.getMinutes())}`;
}

/** A draft's `YYYY-MM-DDTHH:MM`, or a `YYYY-MM-DD`, as a local `Date`. */
export function fromWallClock(s: string): Date {
  if (s.length === 10) return parseDay(s);
  const [date, time] = s.split("T");
  const [y, m, d] = date.split("-").map(Number);
  const [h, min] = time.split(":").map(Number);
  return new Date(y, m - 1, d, h, min);
}

/** A new, empty draft from `start` to `end`. */
export function newDraft(start: Date, end: Date, allDay: boolean, timeZone: string): CalendarDraft {
  return {
    title: "",
    start: allDay ? dayKey(start) : wallClock(start),
    end: allDay ? dayKey(end) : wallClock(end),
    all_day: allDay,
    time_zone: timeZone,
    location: null,
    description: null,
    attendees: [],
    repeat: null,
    busy: true,
  };
}

/**
 * The draft that edits `e`: its times in the window's zone, and everyone on
 * it but the user, who is there by being its organizer.
 */
export function draftFrom(e: CalendarEvent, timeZone: string): CalendarDraft {
  const { start, end } = bounds(e);
  return {
    title: e.title,
    start: e.all_day ? e.start : wallClock(start),
    end: e.all_day ? e.end : wallClock(end),
    all_day: e.all_day,
    time_zone: timeZone,
    location: e.location,
    description: e.description,
    attendees: e.attendees
      .filter((a) => !a.self)
      .map((a) => ({ email: a.email, name: a.name })),
    repeat: e.repeat_rule,
    busy: e.busy || e.attendees.some((a) => a.self && a.response === "declined"),
  };
}

/** The ways the editor offers to repeat; `custom` keeps a rule it can't say. */
export type RepeatChoice = "none" | "daily" | "weekdays" | "weekly" | "monthly" | "yearly" | "custom";

const BYDAY = ["SU", "MO", "TU", "WE", "TH", "FR", "SA"];

/** A rule's parts, `FREQ=WEEKLY;BYDAY=MO` as `{FREQ: "WEEKLY", BYDAY: "MO"}`. */
function parts(rule: string): Record<string, string> {
  return Object.fromEntries(
    rule
      .replace(/^RRULE:/i, "")
      .split(";")
      .filter(Boolean)
      .map((p) => {
        const [k, v = ""] = p.split("=");
        return [k.toUpperCase(), v];
      }),
  );
}

/** Which choice a rule is, for a Calendar event starting on `start`. */
export function repeatChoice(rule: string | null, start: Date): RepeatChoice {
  if (!rule) return "none";
  const { FREQ, BYDAY: byday, BYMONTHDAY, INTERVAL, BYMONTH, BYSETPOS, COUNT, ...rest } =
    parts(rule);
  const extra = Object.keys(rest).filter((k) => k !== "UNTIL" && k !== "WKST");
  if ((INTERVAL && INTERVAL !== "1") || BYMONTH || BYSETPOS || COUNT || extra.length) return "custom";
  switch (FREQ) {
    case "DAILY":
      return byday ? "custom" : "daily";
    case "WEEKLY":
      if (byday === "MO,TU,WE,TH,FR") return "weekdays";
      return !byday || byday === BYDAY[start.getDay()] ? "weekly" : "custom";
    case "MONTHLY":
      return !byday && (!BYMONTHDAY || BYMONTHDAY === String(start.getDate())) ? "monthly" : "custom";
    case "YEARLY":
      return byday || BYMONTHDAY ? "custom" : "yearly";
  }
  return "custom";
}

/** The rule for a choice, for a Calendar event starting on `start`, until `until` if given. */
export function ruleFor(choice: RepeatChoice, start: Date, until: string | null): string | null {
  const base: Record<Exclude<RepeatChoice, "none" | "custom">, string> = {
    daily: "FREQ=DAILY",
    weekdays: "FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR",
    weekly: `FREQ=WEEKLY;BYDAY=${BYDAY[start.getDay()]}`,
    monthly: `FREQ=MONTHLY;BYMONTHDAY=${start.getDate()}`,
    yearly: "FREQ=YEARLY",
  };
  if (choice === "none" || choice === "custom") return null;
  return until ? `${base[choice]};UNTIL=${until.replaceAll("-", "")}` : base[choice];
}

/** The last day a rule repeats, as `YYYY-MM-DD`, if it says. */
export function untilOf(rule: string | null): string | null {
  const u = rule ? parts(rule).UNTIL : undefined;
  return u && u.length >= 8 ? `${u.slice(0, 4)}-${u.slice(4, 6)}-${u.slice(6, 8)}` : null;
}

/** A rule with its last day changed, or taken off. */
export function withUntil(rule: string, until: string | null): string {
  const kept = rule
    .replace(/^RRULE:/i, "")
    .split(";")
    .filter((p) => p && !/^UNTIL=/i.test(p) && !/^COUNT=/i.test(p));
  if (until) kept.push(`UNTIL=${until.replaceAll("-", "")}`);
  return kept.join(";");
}

/** How a choice reads in the editor, for a Calendar event starting on `start`. */
export function repeatLabel(choice: RepeatChoice, start: Date, locale?: string): string {
  switch (choice) {
    case "none":
      return "Doesn't repeat";
    case "daily":
      return "Every day";
    case "weekdays":
      return "Every weekday";
    case "weekly":
      return `Every week on ${start.toLocaleDateString(locale, { weekday: "long" })}`;
    case "monthly":
      return `Every month on the ${ordinal(start.getDate())}`;
    case "yearly":
      return `Every year on ${start.toLocaleDateString(locale, { month: "long", day: "numeric" })}`;
    case "custom":
      return "Custom (kept as it is)";
  }
}

function ordinal(n: number): string {
  const s = ["th", "st", "nd", "rd"];
  const v = n % 100;
  return n + (s[(v - 20) % 10] || s[v] || s[0]);
}

const EMAIL = /^[^\s@<>,;]+@[^\s@<>,;]+\.[^\s@<>,;]+$/;

export function isEmail(s: string): boolean {
  return EMAIL.test(s.trim());
}

/**
 * Guests typed or pasted: addresses split by commas, semicolons or spaces,
 * each maybe written `Ada Lovelace <ada@example.com>`. What isn't an address
 * comes back as `rest`, to stay in the box.
 */
export function parseGuests(text: string): {
  guests: { email: string; name: string | null }[];
  rest: string;
} {
  const guests: { email: string; name: string | null }[] = [];
  let rest = text;
  for (const m of text.matchAll(/(?:"?([^"<,;]*?)"?\s*)?<([^>\s]+@[^>\s]+)>/g)) {
    guests.push({ email: m[2].trim(), name: m[1]?.trim() || null });
    rest = rest.replace(m[0], " ");
  }
  const left: string[] = [];
  for (const word of rest.split(/[\s,;]+/).filter(Boolean)) {
    if (isEmail(word)) guests.push({ email: word, name: null });
    else left.push(word);
  }
  return { guests, rest: left.join(" ") };
}

/** Minutes rounded to the grid's step. */
export function snap(minutes: number, step = 15): number {
  return Math.round(minutes / step) * step;
}

/**
 * A timed span made by dragging down a day's column from `a` to `b`
 * minutes, at least one step long; a click with no drag makes half an hour.
 */
export function spanOnDay(day: Date, a: number, b: number, step = 15): { start: Date; end: Date } {
  const moved = Math.abs(b - a) >= step / 2;
  // A click lands in the slot it's in; a drag snaps both its ends.
  const first = moved ? snap(Math.min(a, b), step) : Math.floor(a / step) * step;
  const top = Math.max(0, Math.min(first, 24 * 60 - step));
  const bottom = moved ? Math.min(24 * 60, Math.max(snap(Math.max(a, b), step), top + step)) : top + 30;
  const base = startOfDay(day);
  return {
    start: new Date(base.getFullYear(), base.getMonth(), base.getDate(), 0, top),
    end: new Date(base.getFullYear(), base.getMonth(), base.getDate(), 0, bottom),
  };
}

/**
 * Where a Calendar event lands after a drag: moved `minutes` and `days`, or
 * with `resize` only its end moved, never shorter than one step. Days move
 * by calendar date, so the clock time holds across a change of clocks.
 */
export function dragged(
  e: CalendarEvent,
  minutes: number,
  days: number,
  resize = false,
  step = 15,
): { start: Date; end: Date } {
  const { start, end } = bounds(e);
  const m = snap(minutes, step);
  const shift = (d: Date, mins: number) => {
    const moved = addDays(d, days);
    return new Date(
      moved.getFullYear(),
      moved.getMonth(),
      moved.getDate(),
      d.getHours(),
      d.getMinutes() + mins,
    );
  };
  if (e.all_day) {
    return { start: addDays(start, days), end: addDays(end, days) };
  }
  if (resize) {
    const newEnd = new Date(end.getTime() + m * MINUTE);
    return {
      start,
      end: newEnd.getTime() - start.getTime() < step * MINUTE ? new Date(start.getTime() + step * MINUTE) : newEnd,
    };
  }
  return { start: shift(start, m), end: shift(end, m) };
}

/** A draft of `e` moved to `start`..`end`. */
export function movedDraft(e: CalendarEvent, start: Date, end: Date, timeZone: string): CalendarDraft {
  const d = draftFrom(e, timeZone);
  return e.all_day
    ? { ...d, start: dayKey(start), end: dayKey(end) }
    : { ...d, start: wallClock(start), end: wallClock(end) };
}

/** What saving a draft says on its button: sending invitations, or not. */
export function saveLabel(guests: number, existing: boolean): string {
  if (guests === 0) return "Save";
  return existing ? "Save and send updates" : "Save and send invites";
}

/** The draft's start moved to `start` (same form), keeping its length. */
export function withStart(d: CalendarDraft, start: string): CalendarDraft {
  const length = fromWallClock(d.end).getTime() - fromWallClock(d.start).getTime();
  const s = fromWallClock(start);
  if (d.all_day) {
    const days = Math.max(1, Math.round(length / (24 * 60 * MINUTE)));
    return { ...d, start: dayKey(s), end: dayKey(addDays(s, days)) };
  }
  return { ...d, start: wallClock(s), end: wallClock(new Date(s.getTime() + Math.max(length, 0))) };
}

/** The draft ending at `end`, or a step after its start if that's earlier. */
export function withEnd(d: CalendarDraft, end: string): CalendarDraft {
  const s = fromWallClock(d.start);
  const e = fromWallClock(end);
  if (d.all_day) return { ...d, end: dayKey(e <= s ? addDays(s, 1) : e) };
  return { ...d, end: wallClock(e <= s ? new Date(s.getTime() + 15 * MINUTE) : e) };
}

/** The last day an all-day draft covers: its end is the day after. */
export function lastDay(d: CalendarDraft): string {
  return dayKey(addDays(fromWallClock(d.end), -1));
}

/** The draft made all-day, or timed again: an hour from 9:00 on its first day. */
export function withAllDay(d: CalendarDraft, on: boolean): CalendarDraft {
  if (on === d.all_day) return d;
  const s = fromWallClock(d.start);
  if (on) {
    const e = fromWallClock(d.end);
    const last = e.getHours() === 0 && e.getMinutes() === 0 ? addDays(e, -1) : e;
    return { ...d, all_day: true, start: dayKey(s), end: dayKey(addDays(startOfDay(last < s ? s : last), 1)) };
  }
  const nine = new Date(s.getFullYear(), s.getMonth(), s.getDate(), 9);
  return { ...d, all_day: false, start: wallClock(nine), end: wallClock(new Date(nine.getTime() + 60 * MINUTE)) };
}
