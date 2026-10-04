<script lang="ts">
  /**
   * A small month to jump by: today ringed in the accent, the days on screen
   * shaded, and its own arrows that page months without moving the view.
   */
  import { calendar } from "./calendar.svelte";
  import { addMonths, monthWeeks, sameDay, viewRange } from "./layout";

  /** The month shown here, which follows the view until its arrows page it. */
  let month = $state(new Date(0));
  $effect(() => {
    month = new Date(calendar.cursor.getFullYear(), calendar.cursor.getMonth(), 1);
  });

  const weeks = $derived(monthWeeks(month, calendar.weekStart));
  const range = $derived(viewRange(calendar.view, calendar.cursor, calendar.weekStart));
  const label = $derived(month.toLocaleDateString(undefined, { month: "long", year: "numeric" }));
  const dows = $derived(
    weeks[0]?.map((d) => d.toLocaleDateString(undefined, { weekday: "narrow" })) ?? [],
  );

  const inView = (d: Date) => d >= range.from && d < range.to;
</script>

<div class="mini">
  <div class="top">
    <span class="label">{label}</span>
    <button class="btn btn-ghost btn-icon nav" aria-label="Previous month" onclick={() => (month = addMonths(month, -1))}>
      <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true"><path d="M10 3 5 8l5 5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" /></svg>
    </button>
    <button class="btn btn-ghost btn-icon nav" aria-label="Next month" onclick={() => (month = addMonths(month, 1))}>
      <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true"><path d="m6 3 5 5-5 5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" /></svg>
    </button>
  </div>
  <div class="days" role="grid" aria-label={label}>
    {#each dows as d}<span class="dow" aria-hidden="true">{d}</span>{/each}
    {#each weeks as week}
      {#each week as day, i (day.getTime())}
        {@const on = inView(day)}
        <button
          class="day"
          class:other={day.getMonth() !== month.getMonth()}
          class:today={sameDay(day, calendar.now)}
          class:on
          class:first={on && (i === 0 || !inView(week[i - 1]))}
          class:last={on && (i === 6 || !inView(week[i + 1]))}
          aria-current={sameDay(day, calendar.now) ? "date" : undefined}
          aria-label={day.toLocaleDateString(undefined, { dateStyle: "full" })}
          onclick={() => calendar.goto(day)}
        >
          <span>{day.getDate()}</span>
        </button>
      {/each}
    {/each}
  </div>
</div>

<style>
  .mini {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    user-select: none;
  }

  .top {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    padding-left: var(--space-3);
  }

  .label {
    flex: 1;
    font-size: var(--text-sm);
    font-weight: var(--weight-semibold);
  }

  .nav {
    width: 1.4rem;
    height: 1.4rem;
  }

  .days {
    display: grid;
    grid-template-columns: repeat(7, 1fr);
    row-gap: 2px;
  }

  .dow {
    text-align: center;
    font-size: var(--text-3xs);
    color: var(--fg-muted);
    padding-bottom: var(--space-2);
  }

  .day {
    aspect-ratio: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    border: none;
    background: none;
    padding: 0;
    color: var(--fg);
    font: inherit;
    font-size: var(--text-2xs);
    font-variant-numeric: tabular-nums;
    cursor: pointer;
  }

  .day span {
    width: 1.55rem;
    height: 1.55rem;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: 50%;
  }

  .day:hover span {
    background: var(--hover);
  }

  .day.other {
    color: color-mix(in srgb, var(--fg-muted) 70%, transparent);
  }

  /* The days on screen, as one band. */
  .day.on {
    background: color-mix(in srgb, var(--accent) 12%, transparent);
  }

  .day.first {
    border-top-left-radius: var(--radius-pill);
    border-bottom-left-radius: var(--radius-pill);
  }

  .day.last {
    border-top-right-radius: var(--radius-pill);
    border-bottom-right-radius: var(--radius-pill);
  }

  .day.today span {
    background: var(--accent);
    color: var(--on-accent);
    font-weight: var(--weight-semibold);
  }
</style>
