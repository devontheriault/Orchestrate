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
import { api, events, LOCAL, type CalendarEvent, type CalendarOverview } from "$lib/api";
import { hosts } from "$lib/state/hosts.svelte";
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

type Prefs = { view?: View; shown?: Record<string, boolean> };

/** How often the current-time line moves. */
const TICK_MS = 30_000;

/** The detail open over a Calendar event, and the box it opened from. */
export type Selection = { event: CalendarEvent; anchor: DOMRect };

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
  accounts = $state<null | "list" | "google" | "caldav">(null);
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

  private readLocale() {
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
    } catch {
      // A preference we can't read is one not made.
    }
  }

  private savePrefs() {
    try {
      localStorage.setItem(PREFS, JSON.stringify({ view: this.view, shown: this.shown }));
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

  /** Start signing in to Google, and open the user's browser on it. */
  async connectGoogle(open: (url: string) => Promise<void>) {
    this.signInError = null;
    try {
      const url = await api.calendarConnectGoogle(this.host);
      this.signingIn = true;
      await open(url);
    } catch (e) {
      this.signingIn = false;
      this.signInError = String(e);
    }
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
