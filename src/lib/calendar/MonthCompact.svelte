<script lang="ts">
  /**
   * The month on a phone: a grid of dates with a dot for each Calendar event,
   * which a seventh of a phone has room for where a title doesn't, and under
   * it the list of the day picked. A press on a date picks it; one in the
   * month before or after goes there.
   */
  import { calendar } from "./calendar.svelte";
  import { agendaFor, monthWeeks, sameDay } from "./layout";
  import Agenda from "./Agenda.svelte";

  /** The dots under a date before they stop counting. */
  const DOTS = 3;

  const weeks = $derived(monthWeeks(calendar.cursor, calendar.weekStart));
  const dows = $derived(
    weeks[0]?.map((d) => d.toLocaleDateString(undefined, { weekday: "narrow" })) ?? [],
  );
  const dots = $derived(
    weeks.map((w) => w.map((d) => agendaFor(calendar.visible, d).map((r) => calendar.color(r.event)))),
  );
  const picked = $derived(calendar.cursor);
</script>

<div class="month">
  <div class="grid">
    <div class="dows" aria-hidden="true">
      {#each dows as d}<span>{d}</span>{/each}
    </div>
    {#each weeks as week, w (week[0].getTime())}
      <div class="week">
        {#each week as day, i (day.getTime())}
          {@const colors = dots[w][i]}
          <button
            class="date"
            class:other={day.getMonth() !== calendar.cursor.getMonth()}
            class:today={sameDay(day, calendar.now)}
            class:picked={sameDay(day, picked)}
            aria-pressed={sameDay(day, picked)}
            aria-label={`${day.toLocaleDateString(undefined, { weekday: "long", month: "long", day: "numeric" })}, ${colors.length || "no"} event${colors.length === 1 ? "" : "s"}`}
            onclick={() => calendar.goto(day)}
          >
            <span class="num">{day.getDate()}</span>
            <span class="dots" aria-hidden="true">
              {#each colors.slice(0, DOTS) as c}<span class="dot" style:--cal={c}></span>{/each}
            </span>
          </button>
        {/each}
      </div>
    {/each}
  </div>

  <h2 class:today={sameDay(picked, calendar.now)}>
    {picked.toLocaleDateString(undefined, { weekday: "long", month: "long", day: "numeric" })}
  </h2>
  <Agenda days={[picked]} heads={false} />
</div>

<style>
  .month {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .grid {
    flex: none;
    padding: 0 var(--space-3) var(--space-3);
    border-bottom: 1px solid var(--border);
  }

  .dows,
  .week {
    display: grid;
    grid-template-columns: repeat(7, minmax(0, 1fr));
  }

  .dows span {
    padding: var(--space-3) 0 var(--space-2);
    text-align: center;
    font-size: var(--text-2xs);
    font-weight: var(--weight-semibold);
    color: var(--fg-muted);
  }

  .date {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 3px;
    padding: var(--space-2) 0;
    border: none;
    background: none;
    color: var(--fg);
    font: inherit;
    cursor: pointer;
    -webkit-tap-highlight-color: transparent;
  }

  .num {
    width: 2.1rem;
    height: 2.1rem;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius-pill);
    font-size: var(--text-md);
    font-variant-numeric: tabular-nums;
    transition: background var(--transition-fast);
  }

  .other .num {
    color: color-mix(in srgb, var(--fg-muted) 60%, transparent);
  }

  .today .num {
    color: var(--accent);
    font-weight: var(--weight-semibold);
  }

  .picked .num {
    background: var(--fg);
    color: var(--surface);
    font-weight: var(--weight-semibold);
  }

  .picked.today .num {
    background: var(--accent);
    color: var(--on-accent);
  }

  .date:focus-visible {
    outline: none;
  }

  .date:focus-visible .num {
    box-shadow: var(--focus-ring);
  }

  .dots {
    display: flex;
    gap: 3px;
    height: 5px;
  }

  .dot {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--cal, var(--accent));
  }

  .other .dot {
    opacity: 0.5;
  }

  h2 {
    margin: 0;
    padding: var(--space-5) var(--pad-x) var(--space-2);
    font-size: var(--text-sm);
    font-weight: var(--weight-semibold);
    color: var(--fg-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  h2.today {
    color: var(--accent);
  }
</style>
