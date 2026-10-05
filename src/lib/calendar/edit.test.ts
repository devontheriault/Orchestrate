/** Run with `npm test`, in Halifax time like `layout.test.ts`. */
process.env.TZ = "America/Halifax";

import { test } from "node:test";
import assert from "node:assert/strict";
import type { CalendarEvent } from "$lib/api";
import {
  draftFrom,
  dragged,
  fromWallClock,
  lastDay,
  movedDraft,
  newDraft,
  parseGuests,
  repeatChoice,
  repeatLabel,
  ruleFor,
  saveLabel,
  spanOnDay,
  untilOf,
  wallClock,
  withAllDay,
  withEnd,
  withStart,
  withUntil,
} from "./edit.ts";

function ev(over: Partial<CalendarEvent>): CalendarEvent {
  return {
    id: "x",
    account_id: "a",
    calendar_id: "c",
    title: "Review",
    start: new Date(2026, 9, 5, 10, 0).toISOString(),
    end: new Date(2026, 9, 5, 11, 0).toISOString(),
    all_day: false,
    time_zone: null,
    location: "Room 4",
    description: null,
    attendees: [],
    organizer: null,
    meeting_url: null,
    link: null,
    recurring: false,
    tentative: false,
    busy: true,
    uid: "u",
    occurrence: "",
    repeat_rule: null,
    can_edit: true,
    draft_id: null,
    draft_note: null,
    ...over,
  };
}

test("wall-clock times round-trip", () => {
  const d = new Date(2026, 10, 1, 9, 30);
  assert.equal(wallClock(d), "2026-11-01T09:30");
  assert.equal(fromWallClock("2026-11-01T09:30").getTime(), d.getTime());
  assert.equal(fromWallClock("2026-11-01").getHours(), 0);
});

test("an existing Calendar event as a draft leaves the user out of the guests", () => {
  const d = draftFrom(
    ev({
      attendees: [
        { name: "Me", email: "me@x.com", response: "accepted", self: true, organizer: true },
        { name: "Ada", email: "ada@x.com", response: null, self: false, organizer: false },
      ],
      repeat_rule: "FREQ=WEEKLY;BYDAY=MO",
    }),
    "America/Halifax",
  );
  assert.equal(d.start, "2026-10-05T10:00");
  assert.equal(d.end, "2026-10-05T11:00");
  assert.deepEqual(d.attendees, [{ email: "ada@x.com", name: "Ada" }]);
  assert.equal(d.repeat, "FREQ=WEEKLY;BYDAY=MO");
  const allDay = draftFrom(ev({ all_day: true, start: "2026-10-05", end: "2026-10-07" }), "UTC");
  assert.deepEqual([allDay.start, allDay.end], ["2026-10-05", "2026-10-07"]);
});

test("repeat choices read and write their rules", () => {
  const monday = new Date(2026, 9, 5);
  assert.equal(repeatChoice(null, monday), "none");
  assert.equal(repeatChoice("FREQ=WEEKLY;BYDAY=MO", monday), "weekly");
  assert.equal(repeatChoice("FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR;UNTIL=20261231", monday), "weekdays");
  assert.equal(repeatChoice("FREQ=WEEKLY;BYDAY=TU", monday), "custom");
  assert.equal(repeatChoice("FREQ=WEEKLY;INTERVAL=2;BYDAY=MO", monday), "custom");
  assert.equal(repeatChoice("FREQ=MONTHLY;BYMONTHDAY=5", monday), "monthly");
  assert.equal(ruleFor("weekly", monday, null), "FREQ=WEEKLY;BYDAY=MO");
  assert.equal(ruleFor("monthly", monday, "2026-12-31"), "FREQ=MONTHLY;BYMONTHDAY=5;UNTIL=20261231");
  assert.equal(ruleFor("none", monday, null), null);
  assert.equal(untilOf("FREQ=DAILY;UNTIL=20261231T035959Z"), "2026-12-31");
  assert.equal(withUntil("FREQ=DAILY;COUNT=4", "2026-11-01"), "FREQ=DAILY;UNTIL=20261101");
  assert.equal(withUntil("FREQ=DAILY;UNTIL=20261101", null), "FREQ=DAILY");
  assert.equal(repeatLabel("monthly", new Date(2026, 9, 22), "en-US"), "Every month on the 22nd");
  assert.equal(repeatLabel("weekly", monday, "en-US"), "Every week on Monday");
});

test("guests typed or pasted", () => {
  assert.deepEqual(parseGuests('Ada Lovelace <ada@x.com>, bob@y.org; not-one'), {
    guests: [
      { email: "ada@x.com", name: "Ada Lovelace" },
      { email: "bob@y.org", name: null },
    ],
    rest: "not-one",
  });
  assert.deepEqual(parseGuests("half@").guests, []);
});

test("a click makes half an hour, a drag its own span, snapped", () => {
  const day = new Date(2026, 9, 5);
  const click = spanOnDay(day, 9 * 60 + 13, 9 * 60 + 14);
  assert.deepEqual([wallClock(click.start), wallClock(click.end)], ["2026-10-05T09:00", "2026-10-05T09:30"]);
  const drag = spanOnDay(day, 14 * 60 + 50, 13 * 60 + 2);
  assert.deepEqual([wallClock(drag.start), wallClock(drag.end)], ["2026-10-05T13:00", "2026-10-05T14:45"]);
  const late = spanOnDay(day, 23 * 60 + 55, 23 * 60 + 55);
  assert.equal(wallClock(late.start), "2026-10-05T23:45");
});

test("a dragged Calendar event keeps its clock time across the change of clocks", () => {
  const e = ev({
    start: new Date(2026, 9, 30, 9, 0).toISOString(),
    end: new Date(2026, 9, 30, 10, 0).toISOString(),
  });
  const moved = dragged(e, 32, 3);
  assert.deepEqual([wallClock(moved.start), wallClock(moved.end)], ["2026-11-02T09:30", "2026-11-02T10:30"]);
  const resized = dragged(e, -70, 0, true);
  assert.deepEqual([wallClock(resized.start), wallClock(resized.end)], ["2026-10-30T09:00", "2026-10-30T09:15"]);
  const d = movedDraft(e, moved.start, moved.end, "America/Halifax");
  assert.equal(d.start, "2026-11-02T09:30");
});

test("an all-day Calendar event moves by days", () => {
  const e = ev({ all_day: true, start: "2026-10-05", end: "2026-10-07" });
  const m = dragged(e, 300, 2);
  const d = movedDraft(e, m.start, m.end, "UTC");
  assert.deepEqual([d.start, d.end], ["2026-10-07", "2026-10-09"]);
});

test("the save button says when it sends invitations", () => {
  assert.equal(saveLabel(0, false), "Save");
  assert.equal(saveLabel(2, false), "Save and send invites");
  assert.equal(saveLabel(1, true), "Save and send updates");
  assert.equal(newDraft(new Date(2026, 9, 5, 9), new Date(2026, 9, 5, 10), false, "UTC").start, "2026-10-05T09:00");
});

test("moving the start keeps the length; the end can't go before it", () => {
  const d = newDraft(new Date(2026, 9, 5, 9), new Date(2026, 9, 5, 10, 30), false, "UTC");
  const moved = withStart(d, "2026-10-06T14:00");
  assert.deepEqual([moved.start, moved.end], ["2026-10-06T14:00", "2026-10-06T15:30"]);
  assert.equal(withEnd(moved, "2026-10-06T13:00").end, "2026-10-06T14:15");
  const allDay = withAllDay(moved, true);
  assert.deepEqual([allDay.start, allDay.end, lastDay(allDay)], ["2026-10-06", "2026-10-07", "2026-10-06"]);
  const timed = withAllDay(allDay, false);
  assert.deepEqual([timed.start, timed.end], ["2026-10-06T09:00", "2026-10-06T10:00"]);
  const trip = withStart(newDraft(new Date(2026, 9, 5), new Date(2026, 9, 8), true, "UTC"), "2026-10-10");
  assert.deepEqual([trip.start, trip.end], ["2026-10-10", "2026-10-13"]);
});
