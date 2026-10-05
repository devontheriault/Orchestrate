<script lang="ts">
  /**
   * The month view: a week to a row, each day as many Calendar events as
   * fit, then "+3 more", which opens that day. All-day and multi-day ones
   * are bars across their days; timed ones are a dot, a time and a title.
   */
  import type { CalendarEvent } from "$lib/api";
  import { calendar } from "./calendar.svelte";
  import { addDays, bounds, hiddenOn, layoutBars, monthWeeks, sameDay, timeLabel } from "./layout";

  const weeks = $derived(monthWeeks(calendar.cursor, calendar.weekStart));
  const layouts = $derived(weeks.map((w) => layoutBars(calendar.visible, w)));

  let weeksHeight = $state(0);
  let laneProbe: HTMLElement | undefined = $state();
  /** A lane's height in pixels, read off the probe so it follows the root size. */
  let lanePx = $state(22);
  $effect(() => {
    if (laneProbe) lanePx = laneProbe.offsetHeight || lanePx;
  });

  /** How many lanes fit under a day's number. */
  const fits = $derived(
    Math.max(1, Math.floor((weeksHeight / Math.max(weeks.length, 1) - lanePx * 1.35) / lanePx)),
  );

  function shown(lanes: number): number {
    return lanes > fits ? fits - 1 : fits;
  }

  const dows = $derived(weeks[0]?.map((d) => d.toLocaleDateString(undefined, { weekday: "short" })) ?? []);

  function past(e: CalendarEvent): boolean {
    return bounds(e).end <= calendar.now;
  }

  function solid(e: CalendarEvent, span: number): boolean {
    return e.all_day || span > 1;
  }
</script>

<div class="month">
  <div class="dows">
    {#each dows as d}<div>{d}</div>{/each}
  </div>
  <div class="weeks" bind:clientHeight={weeksHeight} style:--weeks={weeks.length}>
    <span class="lane-probe" bind:this={laneProbe} aria-hidden="true"></span>
    {#each weeks as week, w (week[0].getTime())}
      {@const lay = layouts[w]}
      {@const lanes = shown(lay.lanes)}
      <div class="week">
        {#each week as day, i (day.getTime())}
          <!-- A press on a day's empty space makes an all-day Calendar event on it. -->
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <div
            class="cell"
            class:other={day.getMonth() !== calendar.cursor.getMonth()}
            class:weekend={day.getDay() === 0 || day.getDay() === 6}
            style:grid-column={i + 1}
            role="presentation"
            onclick={(e) => {
              if (e.target === e.currentTarget) {
                calendar.create(day, addDays(day, 1), true, e.currentTarget.getBoundingClientRect());
              }
            }}
          >
            <button
              class="num"
              class:today={sameDay(day, calendar.now)}
              onclick={() => calendar.goto(day, "day")}
              title="Show this day"
            >
              {day.getDate() === 1
                ? day.toLocaleDateString(undefined, { month: "short", day: "numeric" })
                : day.getDate()}
            </button>
          </div>
        {/each}
        <div class="bars" style:--lanes={fits}>
          {#each lay.bars.filter((b) => b.lane < lanes) as bar (bar.event.id)}
            {@const isSolid = solid(bar.event, bar.span)}
            <button
              class="item"
              class:solid={isSolid}
              class:past={past(bar.event)}
              class:free={!bar.event.busy}
              class:cut-before={bar.continuesBefore}
              class:cut-after={bar.continuesAfter}
              class:selected={calendar.selected?.event.id === bar.event.id}
              class:draft={!!bar.event.draft_id}
              style:--cal={calendar.color(bar.event)}
              style:grid-column={`${bar.col + 1} / span ${bar.span}`}
              style:grid-row={bar.lane + 1}
              onclick={(e) => calendar.select(bar.event, e.currentTarget)}
            >
              {#if !isSolid}
                <span class="dot"></span>
                <span class="time">{timeLabel(bounds(bar.event).start, calendar.hour12)}</span>
              {/if}
              <span class="title">{bar.event.title || "(No title)"}</span>
            </button>
          {/each}
          {#each week as day, i}
            {@const more = hiddenOn(lay.bars, i, lanes)}
            {#if more > 0}
              <button
                class="more"
                style:grid-column={i + 1}
                style:grid-row={lanes + 1}
                onclick={() => calendar.goto(day, "day")}
              >
                +{more} more
              </button>
            {/if}
          {/each}
        </div>
      </div>
    {/each}
  </div>
</div>

<style>
  .month {
    --line: color-mix(in srgb, var(--border) 70%, transparent);
    --lane: 1.4rem;
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .dows {
    display: grid;
    grid-template-columns: repeat(7, minmax(0, 1fr));
    border-bottom: 1px solid var(--line);
  }

  .dows div {
    padding: var(--space-4) var(--space-4);
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--fg-muted);
  }

  .weeks {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-rows: repeat(var(--weeks), minmax(0, 1fr));
    position: relative;
  }

  .lane-probe {
    position: absolute;
    visibility: hidden;
    height: var(--lane);
  }

  .week {
    position: relative;
    display: grid;
    grid-template-columns: repeat(7, minmax(0, 1fr));
    border-bottom: 1px solid var(--line);
    min-height: 0;
    overflow: hidden;
  }

  .cell {
    grid-row: 1;
    border-left: 1px solid var(--line);
    padding: var(--space-2) var(--space-3);
  }

  .cell:first-child {
    border-left: none;
  }

  .cell {
    cursor: copy;
  }

  .cell.weekend {
    background: color-mix(in srgb, var(--panel-bg) 55%, transparent);
  }

  .cell.other .num {
    color: color-mix(in srgb, var(--fg-muted) 65%, transparent);
  }

  .num {
    min-width: 1.6rem;
    height: 1.6rem;
    padding: 0 0.35rem;
    border: none;
    border-radius: var(--radius-pill);
    background: none;
    color: var(--fg);
    font: inherit;
    font-size: var(--text-sm);
    font-weight: var(--weight-medium);
    cursor: pointer;
  }

  .num:hover {
    background: var(--hover);
  }

  .num.today {
    background: var(--accent);
    color: var(--on-accent);
    font-weight: var(--weight-semibold);
  }

  .bars {
    position: absolute;
    inset: calc(var(--lane) * 1.35) 0 0;
    display: grid;
    grid-template-columns: repeat(7, minmax(0, 1fr));
    grid-template-rows: repeat(var(--lanes), var(--lane));
    row-gap: 1px;
    align-content: start;
    pointer-events: none;
  }

  .item,
  .more {
    pointer-events: auto;
  }

  .item {
    --c: var(--cal, var(--accent));
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
    margin: 0 3px;
    padding: 0 var(--space-3);
    border: none;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--fg);
    font: inherit;
    font-size: var(--text-xs);
    text-align: left;
    cursor: pointer;
  }

  .item:hover {
    background: var(--hover);
  }

  .item.solid {
    background: color-mix(in srgb, var(--c) 22%, var(--surface));
    box-shadow: inset 3px 0 0 var(--c);
    padding-left: calc(var(--space-3) + 3px);
  }

  .item.solid:hover {
    background: color-mix(in srgb, var(--c) 32%, var(--surface));
  }

  .item.solid.free {
    background: color-mix(in srgb, var(--c) 7%, var(--surface));
  }

  .item.selected {
    background: color-mix(in srgb, var(--c) 30%, var(--surface));
    box-shadow:
      inset 3px 0 0 var(--c),
      0 0 0 1.5px var(--c);
  }

  .item.cut-before {
    margin-left: 0;
    border-top-left-radius: 0;
    border-bottom-left-radius: 0;
  }

  .item.cut-after {
    margin-right: 0;
    border-top-right-radius: 0;
    border-bottom-right-radius: 0;
  }

  .item.draft {
    outline: 1.5px dashed var(--c);
    outline-offset: -1.5px;
  }

  .item.past {
    opacity: 0.55;
  }

  .dot {
    flex: none;
    width: 0.45rem;
    height: 0.45rem;
    border-radius: 50%;
    background: var(--c);
  }

  .time {
    flex: none;
    color: var(--fg-muted);
    font-variant-numeric: tabular-nums;
  }

  .title {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .item.solid .title {
    font-weight: var(--weight-medium);
  }

  .more {
    justify-self: start;
    margin: 0 3px;
    padding: 0 var(--space-3);
    border: none;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--fg-muted);
    font: inherit;
    font-size: var(--text-2xs);
    font-weight: var(--weight-semibold);
    cursor: pointer;
  }

  .more:hover {
    color: var(--fg);
    background: var(--hover);
  }
</style>
