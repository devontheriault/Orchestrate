/**
 * The Calendar Space's state: what's on screen, and the Host's calendars it
 * comes from. The Host keeps the accounts and syncs them (ADR 0017); this
 * asks it for the Calendar events in the range on screen, and asks again
 * whenever it says something changed.
 *
 * Nothing here runs until `start()`, from the Space's `onMount`: the page is
 * rendered ahead of time (ADR 0015), and today's date or the user's locale
 * read then would be the build machine's.
 */
import {
  api,
  events,
  LOCAL,
  type CalendarAnswer,
  type CalendarDraft,
  type CalendarEvent,
  type CalendarOverview,
  type CalendarScope,
  type CalendarTarget,
} from "$lib/api";
import { hosts } from "$lib/state/hosts.svelte";
import { draftFrom, movedDraft, newDraft } from "./edit";
import {
  calendarKey,
  localIso,
  startOfDay,
  stepCursor,
  viewRange,
  type View,
} from "./layout";

/** Where the view and the calendars the user hid are kept between launches. */
const PREFS = "orchestrate.calendar";

type Prefs = { view?: View; shown?: Record<string, boolean>; calendar?: string };

/** How often the current-time line moves. */
const TICK_MS = 30_000;

/** The detail open over a Calendar event, and the box it opened from. */
export type Selection = { event: CalendarEvent; anchor: DOMRect };

/**
 * A Calendar event being written: a new one, a change to one (`event`), or
 * an Agent's draft being looked over before it's sent (`draftId`).
 */
export type Editing = {
  draft: CalendarDraft;
  /** The calendar it goes on, by `calendarKey`. */
  calendar: string;
  event: CalendarEvent | null;
  draftId: string | null;
  /** The Agent's reason for a draft, shown while the user looks it over. */
  note?: string | null;
  anchor: DOMRect | null;
};

/** A change waiting on "this one or all of them?", for a repeating Calendar event. */
export type Asking = {
  event: CalendarEvent;
  run: (scope: CalendarScope) => Promise<void>;
  /** What the choice is about: "Change", "Delete", "Answer for". */
  verb: string;
  at: { x: number; y: number };
};

class CalendarSpace {
  started = $state(false);
  /** Moves every half minute, for the current-time line and "today". */
  now = $state(new Date(0));
  /** A day inside what's on screen. */
  cursor = $state(new Date(0));
  view = $state<View>("week");
  /** 0 for Sunday, 1 for Monday: the locale's. */
  weekStart = $state(0);
  hour12 = $state(true);

  overview = $state<CalendarOverview | null>(null);
  events = $state<CalendarEvent[]>([]);
  /** A sync the user asked for is running. */
  syncing = $state(false);
  error = $state<string | null>(null);
  /** What the user turned on or off, by `calendarKey`; the provider's choice otherwise. */
  shown = $state<Record<string, boolean>>({});

  selected = $state<Selection | null>(null);
  /** The accounts dialog, and which part of it to open on. */
  accounts = $state<null | "list" | "google" | "microsoft" | "caldav">(null);
  editing = $state<Editing | null>(null);
  asking = $state<Asking | null>(null);
  /** A write on its way to the provider. */
  saving = $state(false);
  editError = $state<string | null>(null);
  /** The window's own zone, which drafts are written in. */
  timeZone = $state("UTC");
  /** The calendar a new Calendar event goes on unless the user picks another. */
  lastCalendar = $state<string | null>(null);
  /** A Google sign-in waiting on the browser, or why the last one failed. */
  signingIn = $state(false);
  signInError = $state<string | null>(null);

  /** The Host the calendars are on: this machine's, or a phone's first. */
  host = $derived(hosts.home);
  /** Whether this window is on the Host's own machine, where Google can be signed in. */
  onHostMachine = $derived(hosts.own === LOCAL && this.host === LOCAL);

  /** Each calendar's colour and name, by `calendarKey`. */
  calendars = $derived(
    new Map(
      (this.overview?.accounts ?? []).flatMap((a) =>
        a.calendars.map((c) => [
          calendarKey(a.id, c.id),
          { ...c, account: a.name, kind: a.kind },
        ]),
      ),
    ),
  );

  /** The Calendar events of the calendars on show. */
  visible = $derived(
    this.events.filter((e) => this.isShown(e.account_id, e.calendar_id)),
  );

  private seq = 0;
  private stops: (() => void)[] = [];

  isShown(accountId: string, calendarId: string): boolean {
    const key = calendarKey(accountId, calendarId);
    return this.shown[key] ?? this.calendars.get(key)?.selected ?? true;
  }

  color(e: Pick<CalendarEvent, "account_id" | "calendar_id">): string | null {
    return this.calendars.get(calendarKey(e.account_id, e.calendar_id))?.color ?? null;
  }

  start() {
    if (this.started) return;
    this.started = true;
    this.now = new Date();
    this.cursor = startOfDay(this.now);
    this.readLocale();
    this.readPrefs();

    const tick = setInterval(() => (this.now = new Date()), TICK_MS);
    this.stops.push(() => clearInterval(tick));
    const listening = [
      events.onCalendarChanged((host) => {
        if (host !== this.host) return;
        this.signingIn = false;
        void this.load();
      }),
      events.onCalendarSignInFailed((e) => {
        if (e.host !== this.host) return;
        this.signingIn = false;
        this.signInError = e.message;
      }),
    ];
    this.stops.push(() => listening.forEach((p) => p.then((un) => un())));

    void this.load().then(() => this.sync());
  }

  stop() {
    this.stops.splice(0).forEach((s) => s());
    this.started = false;
  }

  /** The calendars the user can add to, primary ones first. */
  writable = $derived(
    (this.overview?.accounts ?? []).flatMap((a) =>
      a.calendars
        .filter((c) => c.writable)
        .map((c) => ({ ...c, key: calendarKey(a.id, c.id), account: a })),
    ),
  );

  /** Everyone on the Calendar events loaded, to suggest as guests. */
  people = $derived(
    [
      ...new Map(
        this.events
          .flatMap((e) => e.attendees)
          .filter((a) => !a.self && a.email)
          .map((a) => [a.email.toLowerCase(), { email: a.email, name: a.name }]),
      ).values(),
    ].sort((a, b) => (a.name ?? a.email).localeCompare(b.name ?? b.email)),
  );

  private readLocale() {
    this.timeZone = Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";
    const locale = new Intl.Locale(navigator.language || "en-US") as Intl.Locale & {
      getWeekInfo?: () => { firstDay: number };
      weekInfo?: { firstDay: number };
    };
    const first = locale.getWeekInfo?.().firstDay ?? locale.weekInfo?.firstDay ?? 7;
    this.weekStart = first % 7;
    this.hour12 =
      new Intl.DateTimeFormat(undefined, { hour: "numeric" }).resolvedOptions().hour12 ?? true;
  }

  private readPrefs() {
    try {
      const prefs = JSON.parse(localStorage.getItem(PREFS) ?? "{}") as Prefs;
      if (prefs.view === "day" || prefs.view === "week" || prefs.view === "month") {
        this.view = prefs.view;
      }
      this.shown = prefs.shown ?? {};
      this.lastCalendar = prefs.calendar ?? null;
    } catch {
      // A preference we can't read is one not made.
    }
  }

  private savePrefs() {
    try {
      localStorage.setItem(
        PREFS,
        JSON.stringify({ view: this.view, shown: this.shown, calendar: this.lastCalendar }),
      );
    } catch {
      // Storage full or off: the choice lasts until the window closes.
    }
  }

  /** Read the accounts and the Calendar events on screen afresh. */
  async load() {
    try {
      this.overview = await api.calendarOverview(this.host);
      this.error = null;
    } catch (e) {
      this.error = String(e);
      return;
    }
    await this.fetchRange();
  }

  /** The Calendar events in the range on screen. A slower answer for a range since left is dropped. */
  async fetchRange() {
    if (!this.started) return;
    const seq = ++this.seq;
    const { from, to } = viewRange(this.view, this.cursor, this.weekStart);
    try {
      const got = await api.calendarEvents(this.host, localIso(from), localIso(to));
      if (seq === this.seq) this.events = got;
    } catch (e) {
      if (seq === this.seq) this.error = String(e);
    }
  }

  /** Ask the Host to sync every account now. */
  async sync() {
    if (this.syncing || !this.overview?.accounts.length) return;
    this.syncing = true;
    try {
      this.overview = await api.calendarSync(this.host);
      await this.fetchRange();
    } catch (e) {
      this.error = String(e);
    } finally {
      this.syncing = false;
    }
  }

  setView(view: View) {
    if (view === this.view) return;
    this.view = view;
    this.selected = null;
    this.savePrefs();
    void this.fetchRange();
  }

  goto(day: Date, view?: View) {
    this.cursor = startOfDay(day);
    this.selected = null;
    if (view && view !== this.view) {
      this.view = view;
      this.savePrefs();
    }
    void this.fetchRange();
  }

  step(dir: -1 | 1) {
    this.goto(stepCursor(this.view, this.cursor, dir));
  }

  today() {
    this.now = new Date();
    this.goto(this.now);
  }

  toggle(accountId: string, calendarId: string) {
    const key = calendarKey(accountId, calendarId);
    this.shown = { ...this.shown, [key]: !this.isShown(accountId, calendarId) };
    this.savePrefs();
  }

  select(event: CalendarEvent, anchor: Element) {
    this.selected =
      this.selected?.event.id === event.id
        ? null
        : { event, anchor: anchor.getBoundingClientRect() };
  }

  /** Start signing in to Google or Microsoft, and open the user's browser on it. */
  async signIn(provider: "google" | "microsoft", open: (url: string) => Promise<void>) {
    this.signInError = null;
    try {
      const url = await api.calendarSignIn(this.host, provider);
      this.signingIn = true;
      await open(url);
    } catch (e) {
      this.signingIn = false;
      this.signInError = String(e);
    }
  }

  /** The calendar a new Calendar event goes on: the last one used, or the first primary. */
  private defaultCalendar(): string | null {
    const keys = this.writable.map((c) => c.key);
    if (this.lastCalendar && keys.includes(this.lastCalendar)) return this.lastCalendar;
    return (this.writable.find((c) => c.primary) ?? this.writable[0])?.key ?? null;
  }

  /** Open the editor on a new Calendar event from `start` to `end`. */
  create(start: Date, end: Date, allDay: boolean, anchor: DOMRect | null) {
    const cal = this.defaultCalendar();
    if (!cal) {
      this.accounts = "list";
      return;
    }
    this.selected = null;
    this.editError = null;
    this.editing = {
      draft: newDraft(start, end, allDay, this.timeZone),
      calendar: cal,
      event: null,
      draftId: null,
      anchor,
    };
  }

  /** Open the editor on a Calendar event, or on an Agent's draft to send. */
  edit(event: CalendarEvent, anchor: DOMRect | null) {
    this.selected = null;
    this.editError = null;
    const key = calendarKey(event.account_id, event.calendar_id);
    this.editing = {
      draft: draftFrom(event, this.timeZone),
      calendar: this.writable.some((c) => c.key === key) ? key : (this.defaultCalendar() ?? key),
      event: event.draft_id ? null : event,
      draftId: event.draft_id,
      note: event.draft_note,
      anchor,
    };
  }

  private target(e: CalendarEvent): CalendarTarget {
    return {
      account_id: e.account_id,
      calendar_id: e.calendar_id,
      uid: e.uid,
      occurrence: e.occurrence,
    };
  }

  /** Run a write, then read the range again; the Host has synced by then. */
  private async writing(run: () => Promise<void>): Promise<boolean> {
    this.saving = true;
    this.editError = null;
    try {
      await run();
      await this.load();
      return true;
    } catch (e) {
      this.editError = String(e).replace(/^Error: /, "");
      return false;
    } finally {
      this.saving = false;
    }
  }

  /** For a repeating Calendar event, ask which occurrences first. */
  private scoped(e: CalendarEvent, verb: string, at: { x: number; y: number }, run: (s: CalendarScope) => Promise<void>) {
    if (e.recurring && e.occurrence) {
      this.asking = { event: e, verb, at, run };
    } else {
      void run("all");
    }
  }

  /** Save what the editor holds. */
  save(at: { x: number; y: number }) {
    const ed = this.editing;
    if (!ed) return;
    const cal = this.writable.find((c) => c.key === ed.calendar);
    if (!cal) {
      this.editError = "Pick a calendar to put it on.";
      return;
    }
    this.lastCalendar = ed.calendar;
    this.savePrefs();
    const draft = { ...ed.draft, title: ed.draft.title.trim() };
    if (!ed.event) {
      void this.writing(async () => {
        await api.calendarCreate(this.host, cal.account.id, cal.id, draft);
        if (ed.draftId) await api.calendarDiscardDraft(this.host, ed.draftId);
      }).then((ok) => {
        if (ok) this.editing = null;
      });
      return;
    }
    const event = ed.event;
    this.scoped(event, "Change", at, async (scope) => {
      // Only one occurrence: it keeps no repeat of its own.
      const d = scope === "this" ? { ...draft, repeat: null } : draft;
      if (await this.writing(() => api.calendarUpdate(this.host, this.target(event), d, scope))) {
        this.editing = null;
      }
    });
  }

  /** Move or stretch a Calendar event, as a drag on the grid does. */
  move(event: CalendarEvent, start: Date, end: Date, at: { x: number; y: number }) {
    const draft = movedDraft(event, start, end, this.timeZone);
    this.scoped(event, "Move", at, async (scope) => {
      const d = scope === "this" ? { ...draft, repeat: null } : draft;
      await this.writing(() => api.calendarUpdate(this.host, this.target(event), d, scope));
      if (this.editError) this.error = this.editError;
    });
  }

  remove(event: CalendarEvent, at: { x: number; y: number }) {
    if (event.draft_id) {
      const id = event.draft_id;
      this.selected = null;
      void this.writing(() => api.calendarDiscardDraft(this.host, id));
      return;
    }
    this.scoped(event, "Delete", at, async (scope) => {
      if (await this.writing(() => api.calendarDelete(this.host, this.target(event), scope))) {
        this.selected = null;
        this.editing = null;
      } else {
        this.error = this.editError;
      }
    });
  }

  respond(event: CalendarEvent, answer: CalendarAnswer, at: { x: number; y: number }) {
    this.scoped(event, "Answer for", at, async (scope) => {
      if (await this.writing(() => api.calendarRespond(this.host, this.target(event), answer, scope))) {
        this.selected = null;
      } else {
        this.error = this.editError;
      }
    });
  }

  /** The user picked which occurrences; run what was waiting on it. */
  choose(scope: CalendarScope | null) {
    const asking = this.asking;
    this.asking = null;
    if (asking && scope) void asking.run(scope);
  }

  async removeAccount(id: string) {
    try {
      await api.calendarRemoveAccount(this.host, id);
    } catch (e) {
      this.error = String(e);
    }
    await this.load();
  }
}

export const calendar = new CalendarSpace();
