/**
 * The Calendar Space's arithmetic: which days a view shows, where each
 * Calendar event sits in them, and what the keyboard does. Pure, so it is
 * tested on its own (`layout.test.ts`) and the components only draw.
 *
 * Days are local `Date`s at midnight, made with the calendar constructor
 * (`new Date(y, m, d)`) rather than by adding milliseconds, so a day that is
 * 23 or 25 hours long, as the clocks change, is still one day. Times inside a
 * day are wall-clock minutes since midnight: a 09:00 Calendar event sits on
 * the 09:00 line whatever the day's length.
 */
import type { CalendarEvent } from "$lib/api";

export type View = "day" | "week" | "month";

export const VIEWS: View[] = ["day", "week", "month"];

const MINUTE = 60_000;

/** Minutes in the day the grid draws. */
export const DAY_MINUTES = 24 * 60;

/** The shortest a Calendar event is drawn, so a zero-length one can be hit. */
export const MIN_MINUTES = 20;

export function startOfDay(d: Date): Date {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate());
}

export function addDays(d: Date, n: number): Date {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() + n);
}

export function addMonths(d: Date, n: number): Date {
  // Clamped, so 31 January plus a month is the end of February.
  const first = new Date(d.getFullYear(), d.getMonth() + n, 1);
  const last = new Date(first.getFullYear(), first.getMonth() + 1, 0).getDate();
  return new Date(first.getFullYear(), first.getMonth(), Math.min(d.getDate(), last));
}

export function sameDay(a: Date, b: Date): boolean {
  return (
    a.getFullYear() === b.getFullYear() &&
    a.getMonth() === b.getMonth() &&
    a.getDate() === b.getDate()
  );
}

/** Calendar days from `a` to `b`, by date rather than by hours. */
export function daysBetween(a: Date, b: Date): number {
  const ua = Date.UTC(a.getFullYear(), a.getMonth(), a.getDate());
  const ub = Date.UTC(b.getFullYear(), b.getMonth(), b.getDate());
  return Math.round((ub - ua) / (24 * 60 * MINUTE));
}

/** `weekStart` is 0 for Sunday, 1 for Monday, and so on. */
export function startOfWeek(d: Date, weekStart: number): Date {
  const back = (d.getDay() - weekStart + 7) % 7;
  return addDays(startOfDay(d), -back);
}

export function dayKey(d: Date): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

/** A `YYYY-MM-DD` as that day's local midnight. */
export function parseDay(key: string): Date {
  const [y, m, d] = key.split("-").map(Number);
  return new Date(y, m - 1, d);
}

/**
 * A local time as RFC 3339 with this machine's offset, not UTC: the Host
 * matches all-day Calendar events by date in the offset it's asked in.
 */
export function localIso(d: Date): string {
  const p = (n: number) => String(Math.trunc(Math.abs(n))).padStart(2, "0");
  const off = -d.getTimezoneOffset();
  const sign = off >= 0 ? "+" : "-";
  return (
    `${dayKey(d)}T${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}` +
    `${sign}${p(off / 60)}:${p(off % 60)}`
  );
}

/** The weeks of the month `month` falls in, each seven days from `weekStart`. */
export function monthWeeks(month: Date, weekStart: number): Date[][] {
  const first = new Date(month.getFullYear(), month.getMonth(), 1);
  const last = new Date(month.getFullYear(), month.getMonth() + 1, 0);
  const weeks: Date[][] = [];
  for (let day = startOfWeek(first, weekStart); day <= last; day = addDays(day, 7)) {
    weeks.push(Array.from({ length: 7 }, (_, i) => addDays(day, i)));
  }
  return weeks;
}

/** The days a view of `cursor` shows, first to last. */
export function viewDays(view: View, cursor: Date, weekStart: number): Date[] {
  switch (view) {
    case "day":
      return [startOfDay(cursor)];
    case "week": {
      const start = startOfWeek(cursor, weekStart);
      return Array.from({ length: 7 }, (_, i) => addDays(start, i));
    }
    case "month":
      return monthWeeks(cursor, weekStart).flat();
  }
}

/** The span to ask the Host for: the view's first midnight to the one after its last day. */
export function viewRange(view: View, cursor: Date, weekStart: number): { from: Date; to: Date } {
  const days = viewDays(view, cursor, weekStart);
  return { from: days[0], to: addDays(days[days.length - 1], 1) };
}

/** Where the previous or next button goes from `cursor`. */
export function stepCursor(view: View, cursor: Date, dir: -1 | 1): Date {
  switch (view) {
    case "day":
      return addDays(cursor, dir);
    case "week":
      return addDays(cursor, 7 * dir);
    case "month":
      return addMonths(cursor, dir);
  }
}

/** A Calendar event's start and end as local times; an all-day one's as midnights. */
export function bounds(e: CalendarEvent): { start: Date; end: Date } {
  if (e.all_day) return { start: parseDay(e.start), end: parseDay(e.end) };
  return { start: new Date(e.start), end: new Date(e.end) };
}

/**
 * Whether a Calendar event goes in the strip across the top of the day and
 * week views rather than in the hours: all-day ones, and timed ones a day or
 * longer, which as boxes would fill whole columns.
 */
export function inTopStrip(e: CalendarEvent): boolean {
  if (e.all_day) return true;
  const { start, end } = bounds(e);
  return end.getTime() - start.getTime() >= 24 * 60 * MINUTE;
}

/** Wall-clock minutes since midnight. */
export function minuteOfDay(d: Date): number {
  return d.getHours() * 60 + d.getMinutes() + d.getSeconds() / 60;
}

/** A Calendar event placed in one day's column of the hours. */
export type Placed = {
  event: CalendarEvent;
  /** Minutes from the top of the day. */
  top: number;
  /** Minutes tall, as drawn. */
  height: number;
  /** Its column among those it overlaps, and how many of them it spans. */
  col: number;
  span: number;
  /** How many columns its overlapping group is divided into. */
  cols: number;
  /** Began the day before, or carries on into the next. */
  continuesBefore: boolean;
  continuesAfter: boolean;
};

/**
 * Lay out the timed Calendar events of `day`, side by side where they
 * overlap. Each overlapping group splits its width into as many columns as
 * it needs at its busiest; an event takes the leftmost free column and then
 * stretches right across any columns nothing beside it is using.
 */
export function layoutDay(events: CalendarEvent[], day: Date): Placed[] {
  const dayStart = startOfDay(day);
  const next = addDays(dayStart, 1);
  const items = events
    .filter((e) => !inTopStrip(e))
    .map((event) => {
      const { start, end } = bounds(event);
      return { event, start, end };
    })
    .filter(({ start, end }) => start < next && (end > dayStart || (end.getTime() === start.getTime() && start >= dayStart)))
    .map(({ event, start, end }) => {
      const top = start < dayStart ? 0 : minuteOfDay(start);
      const bottom = end >= next ? DAY_MINUTES : minuteOfDay(end);
      return {
        event,
        top,
        height: Math.max(bottom - top, 0),
        continuesBefore: start < dayStart,
        continuesAfter: end > next,
      };
    })
    .sort((a, b) => a.top - b.top || b.height - a.height || a.event.title.localeCompare(b.event.title));

  // As drawn: a short one takes up the room of its minimum height.
  const drawnEnd = (i: { top: number; height: number }) => i.top + Math.max(i.height, MIN_MINUTES);

  const placed: Placed[] = [];
  let group: (Placed & { bottom: number })[] = [];
  let groupEnd = -1;
  const flush = () => {
    const cols = Math.max(0, ...group.map((p) => p.col + 1));
    for (const p of group) {
      p.cols = cols;
      // Stretch right over columns no overlapping neighbour uses.
      let span = 1;
      while (
        p.col + span < cols &&
        !group.some(
          (q) => q.col === p.col + span && q.top < p.bottom && p.top < q.bottom,
        )
      ) {
        span++;
      }
      p.span = span;
      const { bottom: _, ...rest } = p;
      placed.push(rest);
    }
    group = [];
  };
  for (const item of items) {
    if (item.top >= groupEnd) {
      flush();
      groupEnd = -1;
    }
    const bottom = drawnEnd(item);
    // The leftmost column whose last occupant has ended.
    let col = 0;
    while (group.some((q) => q.col === col && q.bottom > item.top)) col++;
    group.push({ ...item, col, span: 1, cols: 1, bottom });
    groupEnd = Math.max(groupEnd, bottom);
  }
  flush();
  return placed;
}

/** A Calendar event as a bar across a row of days: the top strip, or a month week. */
export type Bar = {
  event: CalendarEvent;
  /** The first of the row's days it covers, and how many. */
  col: number;
  span: number;
  /** Its line within the row, from the top. */
  lane: number;
  continuesBefore: boolean;
  continuesAfter: boolean;
};

/**
 * Lay `events` out as bars across `days`, a row of consecutive days, each in
 * the first lane free across all its days. Longer ones go first, so a
 * week-long trip runs along the top rather than weaving between one-offs.
 */
export function layoutBars(events: CalendarEvent[], days: Date[]): { bars: Bar[]; lanes: number } {
  if (!days.length) return { bars: [], lanes: 0 };
  const first = startOfDay(days[0]);
  const n = days.length;
  const spans = events
    .map((event) => {
      const { start, end } = bounds(event);
      // The last day it touches: an end at midnight belongs to the day before.
      const lastInstant =
        end.getTime() > start.getTime() ? new Date(end.getTime() - 1) : start;
      const s = daysBetween(first, start);
      const e = daysBetween(first, lastInstant);
      return { event, s, e };
    })
    .filter(({ s, e }) => e >= 0 && s < n)
    .sort(
      (a, b) =>
        a.s - b.s ||
        b.e - b.s - (a.e - a.s) ||
        Number(b.event.all_day) - Number(a.event.all_day) ||
        a.event.start.localeCompare(b.event.start) ||
        a.event.title.localeCompare(b.event.title),
    );
  const taken: boolean[][] = [];
  const bars: Bar[] = [];
  for (const { event, s, e } of spans) {
    const col = Math.max(s, 0);
    const last = Math.min(e, n - 1);
    let lane = 0;
    while (taken[lane]?.slice(col, last + 1).some(Boolean)) lane++;
    taken[lane] ??= Array(n).fill(false);
    for (let i = col; i <= last; i++) taken[lane][i] = true;
    bars.push({
      event,
      col,
      span: last - col + 1,
      lane,
      continuesBefore: s < 0,
      continuesAfter: e > n - 1,
    });
  }
  return { bars, lanes: taken.length };
}

/** How many bars on day `col` sit in lanes from `shown` down: the "+3 more". */
export function hiddenOn(bars: Bar[], col: number, shown: number): number {
  return bars.filter((b) => b.lane >= shown && b.col <= col && col < b.col + b.span).length;
}

/** Where the current-time line sits: minutes from the top of today. */
export function nowLine(now: Date): number {
  return minuteOfDay(now);
}

/** What a key does in the Calendar Space, or null for nothing. */
export type KeyAction =
  | { do: "today" }
  | { do: "view"; view: View }
  | { do: "step"; dir: -1 | 1 }
  | { do: "new" }
  | { do: "close" };

/**
 * The Calendar Space's keys, the ones Google Calendar and Fantastical share:
 * T for today, D/W/M for the view, the arrows (or J and K) through time, and
 * C to make a Calendar event.
 */
export function keyAction(e: {
  key: string;
  ctrlKey?: boolean;
  metaKey?: boolean;
  altKey?: boolean;
}): KeyAction | null {
  if (e.ctrlKey || e.metaKey || e.altKey) return null;
  switch (e.key) {
    case "t":
    case "T":
      return { do: "today" };
    case "d":
    case "D":
      return { do: "view", view: "day" };
    case "w":
    case "W":
      return { do: "view", view: "week" };
    case "m":
    case "M":
      return { do: "view", view: "month" };
    case "ArrowLeft":
    case "k":
    case "K":
    case "p":
      return { do: "step", dir: -1 };
    case "ArrowRight":
    case "j":
    case "J":
    case "n":
      return { do: "step", dir: 1 };
    case "c":
    case "C":
      return { do: "new" };
    case "Escape":
      return { do: "close" };
  }
  return null;
}

/** The heading over a view: "October 2026", "Oct 4 – 10, 2026", "Sunday, October 4". */
export function viewTitle(view: View, cursor: Date, weekStart: number, locale?: string): string {
  if (view === "month") {
    return cursor.toLocaleDateString(locale, { month: "long", year: "numeric" });
  }
  if (view === "day") {
    return cursor.toLocaleDateString(locale, {
      weekday: "long",
      month: "long",
      day: "numeric",
      year: "numeric",
    });
  }
  const days = viewDays("week", cursor, weekStart);
  const range = new Intl.DateTimeFormat(locale, {
    month: "short",
    day: "numeric",
    year: "numeric",
  });
  return range.formatRange(days[0], days[6]);
}

/** A time of day, the way this locale writes one: "9:30 AM" or "09:30". */
export function timeLabel(d: Date, hour12: boolean, locale?: string): string {
  return d.toLocaleTimeString(locale, { hour: "numeric", minute: "2-digit", hour12 });
}

/** An hour on the grid's ruler: "9 AM" or "09:00". */
export function hourLabel(hour: number, hour12: boolean, locale?: string): string {
  const d = new Date(2000, 0, 1, hour);
  return hour12
    ? d.toLocaleTimeString(locale, { hour: "numeric", hour12: true })
    : d.toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit", hour12: false });
}

/**
 * When a Calendar event is, said in full for its detail: "Monday, October 5 ·
 * 9:30 – 10:00 AM", "Oct 3 – 6 · all day".
 */
export function whenLabel(e: CalendarEvent, hour12: boolean, locale?: string): string {
  const { start, end } = bounds(e);
  const dayFmt: Intl.DateTimeFormatOptions = { weekday: "long", month: "long", day: "numeric" };
  if (e.all_day) {
    const last = addDays(end, -1);
    if (sameDay(start, last) || last < start) {
      return `${start.toLocaleDateString(locale, dayFmt)} · all day`;
    }
    const short = new Intl.DateTimeFormat(locale, { month: "short", day: "numeric" });
    return `${short.formatRange(start, last)} · all day`;
  }
  const times = new Intl.DateTimeFormat(locale, { hour: "numeric", minute: "2-digit", hour12 });
  if (sameDay(start, end) || end.getTime() === addDays(startOfDay(start), 1).getTime()) {
    return `${start.toLocaleDateString(locale, dayFmt)} · ${times.formatRange(start, end)}`;
  }
  const full = new Intl.DateTimeFormat(locale, {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
    hour12,
  });
  return full.formatRange(start, end);
}

/** The key a calendar is shown or hidden by: its account and its id. */
export function calendarKey(accountId: string, calendarId: string): string {
  return `${accountId}|${calendarId}`;
}

type Box = { left: number; right: number; top: number; bottom: number };

/**
 * Where a Calendar event's detail opens: beside the box it was opened from,
 * on whichever side has room (the right first), level with its top but kept
 * inside the window. With room on neither side it sits below the box, or
 * above it.
 */
export function placeBeside(
  anchor: Box,
  size: { width: number; height: number },
  view: { width: number; height: number; top: number },
  gap = 8,
): { left: number; top: number } {
  const edge = 8;
  const clampTop = (t: number) =>
    Math.max(view.top + edge, Math.min(t, view.height - size.height - edge));
  const clampLeft = (l: number) => Math.max(edge, Math.min(l, view.width - size.width - edge));
  if (anchor.right + gap + size.width + edge <= view.width) {
    return { left: anchor.right + gap, top: clampTop(anchor.top) };
  }
  if (anchor.left - gap - size.width - edge >= 0) {
    return { left: anchor.left - gap - size.width, top: clampTop(anchor.top) };
  }
  const below = anchor.bottom + gap;
  const top =
    below + size.height + edge <= view.height ? below : anchor.top - gap - size.height;
  return { left: clampLeft(anchor.left), top: clampTop(top) };
}

/** What a meeting link joins, by its host: "Google Meet", "Zoom"… */
export function meetingName(url: string): string {
  const host = (() => {
    try {
      return new URL(url).hostname;
    } catch {
      return "";
    }
  })();
  if (host.endsWith("meet.google.com")) return "Google Meet";
  if (host.endsWith("zoom.us")) return "Zoom";
  if (host.includes("teams.microsoft.com") || host.includes("teams.live.com")) return "Teams";
  if (host.endsWith("webex.com")) return "Webex";
  if (host.endsWith("whereby.com")) return "Whereby";
  if (host.endsWith("meet.jit.si")) return "Jitsi";
  return "the meeting";
}

/** How long ago `at` was, for "Synced 3 min ago". */
export function ago(at: Date, now: Date, locale?: string): string {
  const s = Math.round((now.getTime() - at.getTime()) / 1000);
  if (s < 45) return "just now";
  const fmt = new Intl.RelativeTimeFormat(locale, { numeric: "auto", style: "short" });
  if (s < 3600) return fmt.format(-Math.round(s / 60), "minute");
  if (s < 86_400) return fmt.format(-Math.round(s / 3600), "hour");
  return fmt.format(-Math.round(s / 86_400), "day");
}

/** The UTC offset of zone `tz` at `at`, like "GMT-03:00"; null for a zone Intl doesn't know. */
function offsetIn(tz: string, at: Date): string | null {
  try {
    return (
      new Intl.DateTimeFormat("en-US", { timeZone: tz, timeZoneName: "longOffset" })
        .formatToParts(at)
        .find((p) => p.type === "timeZoneName")?.value ?? null
    );
  } catch {
    return null;
  }
}

/**
 * For a Calendar event made in another zone, when it is there: "7:00 AM in
 * Tokyo". Null when that zone keeps this machine's time, which also covers
 * one zone going by two names (America/Halifax and Canada/Atlantic).
 */
export function zoneNote(
  e: CalendarEvent,
  localZone: string,
  hour12: boolean,
  locale?: string,
): string | null {
  if (!e.time_zone || e.all_day) return null;
  const at = bounds(e).start;
  const theirs = offsetIn(e.time_zone, at);
  if (!theirs || theirs === offsetIn(localZone, at)) return null;
  const time = at.toLocaleTimeString(locale, {
    timeZone: e.time_zone,
    hour: "numeric",
    minute: "2-digit",
    hour12,
  });
  const place = e.time_zone.split("/").pop()!.replace(/_/g, " ");
  return `${time} in ${place}`;
}
