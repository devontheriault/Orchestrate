/**
 * Run with `npm test`. The zone is pinned to Halifax, so the DST cases below
 * fall where they say: the clocks go back there on 1 November 2026.
 */
process.env.TZ = "America/Halifax";

import { test } from "node:test";
import assert from "node:assert/strict";
import type { CalendarEvent } from "$lib/api";
import {
  addMonths,
  agendaFor,
  ago,
  dayKey,
  hiddenOn,
  keyAction,
  layoutBars,
  layoutDay,
  localIso,
  meetingName,
  monthWeeks,
  placeBeside,
  shortViewTitle,
  stepCursor,
  viewDays,
  viewRange,
  whenLabel,
  zoneNote,
} from "./layout.ts";

let n = 0;
function ev(start: string, end: string, title = `e${++n}`): CalendarEvent {
  const all_day = start.length === 10;
  return {
    id: title,
    account_id: "a",
    calendar_id: "c",
    title,
    start,
    end,
    all_day,
    time_zone: null,
    location: null,
    description: null,
    attendees: [],
    organizer: null,
    meeting_url: null,
    link: null,
    recurring: false,
    tentative: false,
    busy: true,
    uid: title,
    occurrence: "",
    repeat_rule: null,
    can_edit: true,
    draft_id: null,
    draft_note: null,
  };
}

/** A local time in Halifax, as the Host would send it (UTC). */
const at = (local: string) => new Date(local).toISOString();

test("the zone is pinned", () => {
  assert.equal(new Date(2026, 9, 5).getTimezoneOffset(), 180);
});

test("a week starts on the locale's first day", () => {
  const sunday = viewDays("week", new Date(2026, 9, 7), 0).map(dayKey);
  assert.deepEqual(sunday, [
    "2026-10-04",
    "2026-10-05",
    "2026-10-06",
    "2026-10-07",
    "2026-10-08",
    "2026-10-09",
    "2026-10-10",
  ]);
  assert.equal(dayKey(viewDays("week", new Date(2026, 9, 4), 1)[0]), "2026-09-28");
});

test("a week across the fall-back still has seven days at midnight", () => {
  const days = viewDays("week", new Date(2026, 10, 1), 0);
  assert.equal(days.length, 7);
  assert.ok(days.every((d) => d.getHours() === 0));
  assert.deepEqual(days.map((d) => d.getDate()), [1, 2, 3, 4, 5, 6, 7]);
});

test("the month grid covers whole weeks", () => {
  const weeks = monthWeeks(new Date(2026, 9, 15), 0);
  assert.equal(weeks.length, 5);
  assert.equal(dayKey(weeks[0][0]), "2026-09-27");
  assert.equal(dayKey(weeks[4][6]), "2026-10-31");
  // February 2026 starts on a Sunday and fills exactly four weeks.
  assert.equal(monthWeeks(new Date(2026, 1, 1), 0).length, 4);
});

test("the range asked for runs to the midnight after the last day, in local offset", () => {
  const { from, to } = viewRange("week", new Date(2026, 9, 28), 0);
  assert.equal(localIso(from), "2026-10-25T00:00:00-03:00");
  assert.equal(localIso(to), "2026-11-01T00:00:00-03:00");
  const after = viewRange("day", new Date(2026, 10, 2), 0);
  assert.equal(localIso(after.from), "2026-11-02T00:00:00-04:00");
});

test("stepping a month clamps to its last day", () => {
  assert.equal(dayKey(addMonths(new Date(2026, 0, 31), 1)), "2026-02-28");
  assert.equal(dayKey(stepCursor("month", new Date(2026, 0, 31), 1)), "2026-02-28");
  assert.equal(dayKey(stepCursor("week", new Date(2026, 9, 28), 1)), "2026-11-04");
});

test("overlapping Calendar events sit side by side", () => {
  const day = new Date(2026, 9, 5);
  const a = ev(at("2026-10-05T09:00"), at("2026-10-05T10:00"), "a");
  const b = ev(at("2026-10-05T09:30"), at("2026-10-05T10:30"), "b");
  const c = ev(at("2026-10-05T10:00"), at("2026-10-05T11:00"), "c");
  const d = ev(at("2026-10-05T13:00"), at("2026-10-05T14:00"), "d");
  const placed = Object.fromEntries(layoutDay([a, b, c, d], day).map((p) => [p.event.title, p]));
  assert.deepEqual(
    Object.fromEntries(
      Object.entries(placed).map(([k, p]) => [k, [p.top, p.height, p.col, p.span, p.cols]]),
    ),
    {
      a: [540, 60, 0, 1, 2],
      b: [570, 60, 1, 1, 2],
      // c starts as a ends, so it takes a's column back.
      c: [600, 60, 0, 1, 2],
      // Alone: the full width.
      d: [780, 60, 0, 1, 1],
    },
  );
});

test("an event stretches across columns nothing beside it uses", () => {
  const day = new Date(2026, 9, 5);
  const long = ev(at("2026-10-05T09:00"), at("2026-10-05T12:00"), "long");
  const x = ev(at("2026-10-05T09:00"), at("2026-10-05T10:00"), "x");
  const y = ev(at("2026-10-05T09:00"), at("2026-10-05T10:00"), "y");
  const late = ev(at("2026-10-05T11:00"), at("2026-10-05T11:30"), "late");
  const placed = Object.fromEntries(layoutDay([long, x, y, late], day).map((p) => [p.event.title, p]));
  assert.equal(placed.long.cols, 3);
  assert.deepEqual([placed.x.col, placed.y.col], [1, 2]);
  // Nothing else is on at 11:00, so it fills the two columns beside `long`.
  assert.deepEqual([placed.late.col, placed.late.span], [1, 2]);
});

test("a short event takes the room it's drawn in", () => {
  const day = new Date(2026, 9, 5);
  const blip = ev(at("2026-10-05T09:00"), at("2026-10-05T09:05"), "blip");
  const next = ev(at("2026-10-05T09:10"), at("2026-10-05T10:00"), "next");
  const placed = layoutDay([blip, next], day);
  assert.equal(placed[0].cols, 2);
});

test("an overnight event is split across its two days", () => {
  const night = ev(at("2026-10-05T22:00"), at("2026-10-06T02:00"), "night");
  const [first] = layoutDay([night], new Date(2026, 9, 5));
  const [second] = layoutDay([night], new Date(2026, 9, 6));
  assert.deepEqual([first.top, first.height, first.continuesAfter], [1320, 120, true]);
  assert.deepEqual([second.top, second.height, second.continuesBefore], [0, 120, true]);
});

test("times sit on the wall clock on the day the clocks go back", () => {
  // 1 November 2026 is 25 hours long in Halifax; 09:00 is still on the 09:00 line.
  const [p] = layoutDay(
    [ev(at("2026-11-01T09:00"), at("2026-11-01T10:00"))],
    new Date(2026, 10, 1),
  );
  assert.deepEqual([p.top, p.height], [540, 60]);
});

test("all-day and day-long events go to the top strip, not the hours", () => {
  const day = new Date(2026, 9, 5);
  const allDay = ev("2026-10-05", "2026-10-06");
  const dayLong = ev(at("2026-10-05T08:00"), at("2026-10-06T09:00"));
  assert.equal(layoutDay([allDay, dayLong], day).length, 0);
});

test("bars pack into lanes, longest first", () => {
  const days = viewDays("week", new Date(2026, 9, 4), 0);
  const trip = ev("2026-10-03", "2026-10-08", "trip");
  const holiday = ev("2026-10-12", "2026-10-13", "thanksgiving next week");
  const one = ev("2026-10-05", "2026-10-06", "one");
  const two = ev("2026-10-05", "2026-10-06", "two");
  const timed = ev(at("2026-10-07T15:00"), at("2026-10-07T16:00"), "timed");
  const { bars, lanes } = layoutBars([one, two, trip, holiday, timed], days);
  const by = Object.fromEntries(bars.map((b) => [b.event.title, b]));
  assert.equal(lanes, 3);
  assert.deepEqual(
    [by.trip.col, by.trip.span, by.trip.lane, by.trip.continuesBefore],
    [0, 4, 0, true],
  );
  assert.deepEqual([by.one.lane, by.two.lane], [1, 2]);
  // The trip still holds the top lane on the 7th.
  assert.deepEqual([by.timed.col, by.timed.lane], [3, 1]);
  assert.equal(by["thanksgiving next week"], undefined);
  assert.equal(hiddenOn(bars, 1, 2), 1);
  assert.equal(hiddenOn(bars, 3, 2), 0);
});

test("an event ending at midnight doesn't spill into the next day", () => {
  const days = viewDays("week", new Date(2026, 9, 4), 0);
  const { bars } = layoutBars([ev(at("2026-10-05T20:00"), at("2026-10-06T00:00"))], days);
  assert.deepEqual([bars[0].col, bars[0].span], [1, 1]);
});

test("keys", () => {
  assert.deepEqual(keyAction({ key: "t" }), { do: "today" });
  assert.deepEqual(keyAction({ key: "m" }), { do: "view", view: "month" });
  assert.deepEqual(keyAction({ key: "ArrowRight" }), { do: "step", dir: 1 });
  assert.deepEqual(keyAction({ key: "k" }), { do: "step", dir: -1 });
  assert.deepEqual(keyAction({ key: "c" }), { do: "new" });
  assert.equal(keyAction({ key: "t", ctrlKey: true }), null);
  assert.equal(keyAction({ key: "x" }), null);
});

test("when a Calendar event is, said in full", () => {
  // ICU spaces its ranges and AM/PM with thin and narrow spaces.
  const label = (e: CalendarEvent) => whenLabel(e, true, "en-US").replace(/[\u2009\u202f]/g, " ");
  assert.equal(
    label(ev(at("2026-10-05T09:30"), at("2026-10-05T10:00"))),
    "Monday, October 5 · 9:30 – 10:00 AM",
  );
  assert.equal(label(ev("2026-10-05", "2026-10-06")), "Monday, October 5 · all day");
  assert.equal(label(ev("2026-10-03", "2026-10-07")), "Oct 3 – 6 · all day");
});

test("the detail opens beside its Calendar event, on the side with room", () => {
  const view = { width: 1000, height: 700, top: 40 };
  const size = { width: 300, height: 400 };
  // Room on the right.
  assert.deepEqual(placeBeside({ left: 100, right: 200, top: 100, bottom: 160 }, size, view), {
    left: 208,
    top: 100,
  });
  // None on the right: the left, and kept above the window's bottom.
  assert.deepEqual(placeBeside({ left: 800, right: 900, top: 600, bottom: 640 }, size, view), {
    left: 492,
    top: 292,
  });
  // A full-width bar on a narrow window: below it.
  assert.deepEqual(
    placeBeside({ left: 0, right: 400, top: 60, bottom: 80 }, size, { width: 420, height: 700, top: 40 }),
    { left: 8, top: 88 },
  );
});

test("meeting names", () => {
  assert.equal(meetingName("https://meet.google.com/abc-defg-hij"), "Google Meet");
  assert.equal(meetingName("https://us02web.zoom.us/j/123"), "Zoom");
  assert.equal(meetingName("not a url"), "the meeting");
});

test("how long ago a sync was", () => {
  const now = new Date(2026, 9, 5, 12, 0);
  assert.equal(ago(new Date(2026, 9, 5, 11, 59, 40), now, "en"), "just now");
  assert.equal(ago(new Date(2026, 9, 5, 11, 57), now, "en"), "3 min. ago");
  assert.equal(ago(new Date(2026, 9, 5, 9, 0), now, "en"), "3 hr. ago");
});

test("a zone note only for a zone that keeps other time", () => {
  const tokyo = { ...ev("2026-10-06T22:00:00Z", "2026-10-06T23:00:00Z"), time_zone: "Asia/Tokyo" };
  assert.equal(zoneNote(tokyo, "America/Halifax", true, "en-US")?.replace(/\u202f/g, " "), "7:00 AM in Tokyo");
  const same = { ...tokyo, time_zone: "America/Halifax" };
  assert.equal(zoneNote(same, "Canada/Atlantic", true, "en-US"), null);
});

test("a day's list: all-day ones first, then by start, the longer first", () => {
  const day = new Date(2026, 10, 1);
  const lunch = ev(at("2026-11-01T12:00"), at("2026-11-01T13:00"), "lunch");
  const standup = ev(at("2026-11-01T09:30"), at("2026-11-01T10:00"), "standup");
  const pairing = ev(at("2026-11-01T09:30"), at("2026-11-01T11:00"), "pairing");
  const trip = ev("2026-10-30", "2026-11-03", "trip");
  const overnight = ev(at("2026-10-31T22:00"), at("2026-11-01T02:00"), "overnight");
  const conference = ev(at("2026-10-31T09:00"), at("2026-11-02T17:00"), "conference");
  const tomorrow = ev(at("2026-11-02T00:00"), at("2026-11-02T01:00"), "tomorrow");
  const yesterday = ev("2026-10-31", "2026-11-01", "yesterday");
  const rows = agendaFor([lunch, standup, pairing, trip, overnight, conference, tomorrow, yesterday], day);
  assert.deepEqual(
    rows.map((r) => [r.event.title, r.allDay, r.continuesBefore, r.continuesAfter]),
    [
      ["trip", true, true, true],
      ["conference", true, true, true],
      ["overnight", false, true, false],
      ["pairing", false, false, false],
      ["standup", false, false, false],
      ["lunch", false, false, false],
    ],
  );
});

test("a phone's headings drop what it can do without", () => {
  const now = new Date(2026, 9, 6);
  const title = (view: "day" | "week" | "month", d: Date) =>
    shortViewTitle(view, d, 0, now, "en-US").replace(/[  ]/g, " ");
  assert.equal(title("day", new Date(2026, 9, 6)), "Tue, Oct 6");
  assert.equal(title("week", new Date(2026, 9, 6)), "Oct 4 – 10");
  assert.equal(title("week", new Date(2026, 8, 30)), "Sep 27 – Oct 3");
  assert.equal(title("month", new Date(2026, 9, 6)), "October 2026");
  assert.equal(title("day", new Date(2027, 0, 4)), "Mon, Jan 4, 2027");
});
