<script lang="ts">
  /**
   * Days as a list, the way a phone reads a calendar: each day's Calendar
   * events one under another, with their times and where they are, rather
   * than seven columns of hours too narrow to hold a title. The phone's week
   * is one, and the day picked under its month another.
   */
  import type { CalendarEvent } from "$lib/api";
  import { calendar } from "./calendar.svelte";
  import { agendaFor, sameDay, timeLabel, type AgendaRow } from "./layout";

  let {
    days,
    heads = true,
    scrollToday = false,
  }: {
    days: Date[];
    /** Each day's date over its list; the month's day has its own. */
    heads?: boolean;
    /** Bring today into view when the days change, as the week opens on it. */
    scrollToday?: boolean;
  } = $props();

  const lists = $derived(days.map((d) => agendaFor(calendar.visible, d)));

  let scroller: HTMLElement | undefined = $state();
  let scrolledFor = "";
  $effect(() => {
    const key = `${days[0]?.getTime()}-${days.length}`;
    if (!scroller || key === scrolledFor) return;
    scrolledFor = key;
    const el = scroller;
    requestAnimationFrame(() => {
      const today = scrollToday ? el.querySelector<HTMLElement>(".day.today") : null;
      el.scrollTop = today ? today.offsetTop - el.offsetTop : 0;
    });
  });

  function when(r: AgendaRow): string {
    if (r.allDay) return "All day";
    const start = timeLabel(r.start, calendar.hour12);
    const end = timeLabel(r.end, calendar.hour12);
    if (r.continuesBefore && r.continuesAfter) return "All day";
    if (r.continuesBefore) return `Until ${end}`;
    if (r.continuesAfter) return `From ${start}`;
    return r.end.getTime() === r.start.getTime() ? start : `${start} – ${end}`;
  }

  function declined(e: CalendarEvent): boolean {
    return e.attendees.some((a) => a.self && a.response === "declined");
  }
</script>

<div class="agenda" bind:this={scroller}>
  {#each days as day, i (day.getTime())}
    {@const today = sameDay(day, calendar.now)}
    <section class="day" class:today>
      {#if heads}
        <button class="day-head" onclick={() => calendar.goto(day, "day")} title="Show this day">
          <span class="dow">{day.toLocaleDateString(undefined, { weekday: "short" })}</span>
          <span class="num">{day.getDate()}</span>
        </button>
      {/if}
      <ul>
        {#each lists[i] as r (r.event.id)}
          <li>
            <button
              class="event"
              class:past={r.end <= calendar.now}
              class:free={!r.event.busy}
              class:tentative={r.event.tentative}
              class:declined={declined(r.event)}
              class:draft={!!r.event.draft_id}
              class:selected={calendar.selected?.event.id === r.event.id}
              style:--cal={calendar.color(r.event)}
              onclick={(e) => calendar.select(r.event, e.currentTarget)}
            >
              <span class="title">{r.event.title || "(No title)"}</span>
              <span class="when">
                {when(r)}{#if r.event.location}{` · ${r.event.location}`}{/if}
              </span>
            </button>
          </li>
        {:else}
          <li class="none">Nothing planned</li>
        {/each}
      </ul>
    </section>
  {/each}
</div>

<style>
  .agenda {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: var(--space-2) 0 var(--space-6);
  }

  .day {
    display: grid;
    grid-template-columns: 3.4rem minmax(0, 1fr);
    gap: var(--space-3);
    padding: var(--space-3) var(--pad-x) var(--space-3) var(--space-3);
  }

  .day + .day {
    border-top: 1px solid color-mix(in srgb, var(--border) 60%, transparent);
  }

  /* The month's day has no date beside it: the grid above says which. */
  .day:not(:has(.day-head)) {
    grid-template-columns: minmax(0, 1fr);
    padding-left: var(--pad-x);
  }

  .day-head {
    align-self: start;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    padding: var(--space-2) 0;
    border: none;
    border-radius: var(--radius-md);
    background: none;
    color: var(--fg-muted);
    font: inherit;
    cursor: pointer;
  }

  .dow {
    font-size: var(--text-2xs);
    font-weight: var(--weight-semibold);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .num {
    min-width: 2.1rem;
    height: 2.1rem;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius-pill);
    font-size: var(--text-xl);
    font-weight: var(--weight-medium);
    color: var(--fg);
    font-variant-numeric: tabular-nums;
  }

  .today .dow {
    color: var(--accent);
  }

  .today .num {
    background: var(--accent);
    color: var(--on-accent);
    font-weight: var(--weight-semibold);
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-width: 0;
  }

  .none {
    display: flex;
    align-items: center;
    min-height: 2.9rem;
    color: var(--fg-muted);
    font-size: var(--text-sm);
  }

  .event {
    --c: var(--cal, var(--accent));
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: var(--space-3) var(--space-4) var(--space-3) calc(var(--space-4) + 4px);
    border: none;
    border-radius: var(--radius-md);
    background: color-mix(in srgb, var(--c) 18%, var(--surface));
    box-shadow: inset 4px 0 0 var(--c);
    color: var(--fg);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .event:active,
  .event.selected {
    background: color-mix(in srgb, var(--c) 32%, var(--surface));
  }

  .event:focus-visible {
    outline: var(--focus-outline);
    outline-offset: 1px;
  }

  .event.past:not(.selected) {
    opacity: 0.55;
  }

  /* Marked free: an outline, so it reads as not taking the time. */
  .event.free:not(.selected) {
    background: color-mix(in srgb, var(--c) 6%, var(--surface));
    box-shadow:
      inset 4px 0 0 var(--c),
      inset 0 0 0 1px color-mix(in srgb, var(--c) 45%, transparent);
  }

  .event.tentative:not(.selected) {
    background-image: repeating-linear-gradient(
      -45deg,
      transparent 0 5px,
      color-mix(in srgb, var(--c) 14%, transparent) 5px 10px
    );
  }

  .event.declined .title {
    text-decoration: line-through;
  }

  .event.declined:not(.selected) {
    opacity: 0.45;
  }

  /* An Agent's draft: dashed, as something not yet in the calendar. */
  .event.draft:not(.selected) {
    background: color-mix(in srgb, var(--c) 6%, var(--surface));
    box-shadow: none;
    outline: 1.5px dashed var(--c);
    outline-offset: -1.5px;
  }

  .title {
    font-size: var(--text-md);
    font-weight: var(--weight-semibold);
    line-height: var(--leading-tight);
    overflow-wrap: anywhere;
  }

  .when {
    font-size: var(--text-sm);
    color: color-mix(in srgb, currentColor 70%, transparent);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 100%;
  }
</style>
