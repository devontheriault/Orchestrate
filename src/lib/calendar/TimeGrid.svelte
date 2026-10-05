<script lang="ts">
  /**
   * The day and week views: a column of hours for each day, the Calendar
   * events in them side by side where they overlap, and a strip across the
   * top for the all-day ones. Where everything sits is `layout.ts`'s.
   *
   * Press on empty time to make a Calendar event there (drag for its length),
   * on the strip for an all-day one; drag one to move it, its bottom edge to
   * change its length. Where they land is `edit.ts`'s.
   */
  import type { CalendarEvent } from "$lib/api";
  import { calendar } from "./calendar.svelte";
  import {
    DAY_MINUTES,
    MIN_MINUTES,
    hourLabel,
    inTopStrip,
    layoutBars,
    layoutDay,
    nowLine,
    sameDay,
    timeLabel,
    bounds,
    addDays,
  } from "./layout";
  import { dragged, fromWallClock, spanOnDay } from "./edit";

  let { days }: { days: Date[] } = $props();

  /** The strip's lanes shown before it folds into a "+3". */
  const FOLDED_LANES = 3;

  let scroller: HTMLElement | undefined = $state();
  /** The scroll area's scrollbar, which the header and strip leave room for. */
  let scrollbar = $state(0);
  let hourPx = $state(48);
  let stripOpen = $state(false);

  const placed = $derived(days.map((d) => layoutDay(calendar.visible, d)));
  const strip = $derived(
    layoutBars(
      calendar.visible.filter((e) => inTopStrip(e)),
      days,
    ),
  );
  const shownLanes = $derived(
    stripOpen || strip.lanes <= FOLDED_LANES + 1 ? strip.lanes : FOLDED_LANES,
  );
  const todayCol = $derived(days.findIndex((d) => sameDay(d, calendar.now)));
  const now = $derived(nowLine(calendar.now));
  const hours = Array.from({ length: 24 }, (_, h) => h);

  /** The zone's short name for the corner, like "GMT-3" or "EDT". */
  const zone = $derived(
    new Intl.DateTimeFormat(undefined, { timeZoneName: "short" })
      .formatToParts(calendar.now)
      .find((p) => p.type === "timeZoneName")?.value ?? "",
  );

  // Open on the working day: the hour before now when today is on screen,
  // 07:30 otherwise. Only when the days change, not on every tick.
  let scrolledFor = "";
  $effect(() => {
    const key = `${days[0]?.getTime()}-${days.length}`;
    if (!scroller || key === scrolledFor) return;
    scrolledFor = key;
    const el = scroller;
    const minutes = todayCol >= 0 ? Math.max(now - 90, 0) : 7.5 * 60;
    // After layout: before it the area has no height of its own to scroll.
    requestAnimationFrame(() => {
      const hour = el.querySelector<HTMLElement>(".hour")?.offsetHeight || hourPx;
      el.scrollTop = (minutes / 60) * hour;
    });
  });

  $effect(() => {
    if (!scroller) return;
    const el = scroller;
    const measure = () => {
      scrollbar = el.offsetWidth - el.clientWidth;
      hourPx = el.querySelector<HTMLElement>(".hour")?.offsetHeight || hourPx;
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  });

  const pct = (minutes: number) => `${(minutes / DAY_MINUTES) * 100}%`;

  function past(e: CalendarEvent): boolean {
    return bounds(e).end <= calendar.now;
  }

  function declined(e: CalendarEvent): boolean {
    return e.attendees.some((a) => a.self && a.response === "declined");
  }

  // ---- making one
  /** A new Calendar event being pressed out on day `day`, from `a` to `b` minutes. */
  let making = $state<{ day: number; a: number; b: number } | null>(null);
  let makingEl: HTMLElement | undefined = $state();

  function minuteAt(col: HTMLElement, y: number): number {
    const r = col.getBoundingClientRect();
    return Math.max(0, Math.min(DAY_MINUTES, ((y - r.top) / r.height) * DAY_MINUTES));
  }

  function columnDown(e: PointerEvent, i: number) {
    if (e.button !== 0 || (e.target as HTMLElement).closest(".event")) return;
    const col = e.currentTarget as HTMLElement;
    col.setPointerCapture(e.pointerId);
    const m = minuteAt(col, e.clientY);
    making = { day: i, a: m, b: m };
  }

  function columnMove(e: PointerEvent) {
    if (making) making.b = minuteAt(e.currentTarget as HTMLElement, e.clientY);
  }

  function columnUp() {
    if (!making) return;
    const span = spanOnDay(days[making.day], making.a, making.b);
    const rect = makingEl?.getBoundingClientRect() ?? null;
    making = null;
    calendar.create(span.start, span.end, false, rect);
  }

  /** The new one being pressed out, as it will be made. */
  const pressed = $derived(making ? spanOnDay(days[making.day], making.a, making.b) : null);

  /** The new one in the editor, drawn where it will go. */
  const pending = $derived.by(() => {
    const ed = calendar.editing;
    if (!ed || ed.event || ed.draftId || ed.draft.all_day) return null;
    const start = fromWallClock(ed.draft.start);
    const end = fromWallClock(ed.draft.end);
    const i = days.findIndex((d) => sameDay(d, start));
    if (i < 0) return null;
    return { day: i, start, end, title: ed.draft.title };
  });

  const minutesOf = (d: Date) => d.getHours() * 60 + d.getMinutes();

  // ---- moving one
  type Drag = {
    event: CalendarEvent;
    resize: boolean;
    x0: number;
    y0: number;
    dx: number;
    dy: number;
    /** A day column's width and height, to turn pixels into days and minutes. */
    w: number;
    h: number;
    moved: boolean;
  };
  let drag = $state<Drag | null>(null);
  /** Set by a drag's release, so the click that follows doesn't also open it. */
  let swallowClick = false;

  function eventDown(e: PointerEvent, event: CalendarEvent) {
    if (e.button !== 0 || !event.can_edit || event.draft_id) return;
    const el = e.currentTarget as HTMLElement;
    const col = el.closest(".col") as HTMLElement | null;
    if (!col) return;
    el.setPointerCapture(e.pointerId);
    drag = {
      event,
      resize: (e.target as HTMLElement).classList.contains("grip"),
      x0: e.clientX,
      y0: e.clientY,
      dx: 0,
      dy: 0,
      w: col.offsetWidth,
      h: col.offsetHeight,
      moved: false,
    };
  }

  function eventMove(e: PointerEvent) {
    if (!drag) return;
    drag.dx = e.clientX - drag.x0;
    drag.dy = e.clientY - drag.y0;
    if (Math.abs(drag.dx) > 4 || Math.abs(drag.dy) > 4) drag.moved = true;
  }

  /** Where the dragged one would land now. */
  const landing = $derived.by(() => {
    if (!drag?.moved) return null;
    const minutes = (drag.dy / drag.h) * DAY_MINUTES;
    const shift = drag.resize ? 0 : Math.round(drag.dx / drag.w);
    return { ...dragged(drag.event, minutes, shift, drag.resize), shift };
  });

  function eventUp(e: PointerEvent) {
    const d = drag;
    const land = landing;
    drag = null;
    if (!d?.moved || !land) return;
    swallowClick = true;
    const { start, end } = bounds(d.event);
    if (land.start.getTime() === start.getTime() && land.end.getTime() === end.getTime()) return;
    calendar.move(d.event, land.start, land.end, { x: e.clientX, y: e.clientY });
  }

  function eventClick(e: MouseEvent, event: CalendarEvent) {
    if (swallowClick) {
      swallowClick = false;
      return;
    }
    calendar.select(event, e.currentTarget as Element);
  }

  function weekday(d: Date): string {
    return d.toLocaleDateString(undefined, { weekday: "short" });
  }
</script>

<div class="grid" style:--days={days.length} style:--scrollbar={`${scrollbar}px`}>
  <div class="row head">
    <div class="corner" title="Times are in this machine's zone">{zone}</div>
    {#each days as day, i (day.getTime())}
      <button
        class="day-head"
        class:today={i === todayCol}
        class:weekend={day.getDay() === 0 || day.getDay() === 6}
        onclick={() => calendar.goto(day, "day")}
        title="Show this day"
      >
        <span class="dow">{weekday(day)}</span>
        <span class="num">{day.getDate()}</span>
      </button>
    {/each}
  </div>

  {#if strip.lanes > 0}
    <div class="row allday">
      <div class="corner strip-label">
        {#if strip.lanes > FOLDED_LANES + 1}
          <button
            class="fold"
            onclick={() => (stripOpen = !stripOpen)}
            aria-expanded={stripOpen}
            title={stripOpen ? "Show fewer" : "Show every all-day one"}
          >
            {stripOpen ? "less" : `+${strip.lanes - FOLDED_LANES}`}
          </button>
        {:else}
          all-day
        {/if}
      </div>
      <div class="bars" style:--lanes={shownLanes}>
        {#each days as day, i}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <div
            class="bar-col"
            class:today={i === todayCol}
            style:grid-column={i + 1}
            role="presentation"
            onclick={(e) => calendar.create(day, addDays(day, 1), true, e.currentTarget.getBoundingClientRect())}
          ></div>
        {/each}
        {#each strip.bars.filter((b) => b.lane < shownLanes) as bar (bar.event.id)}
          <button
            class="event bar"
            class:past={past(bar.event)}
            class:free={!bar.event.busy}
            class:tentative={bar.event.tentative}
            class:declined={declined(bar.event)}
            class:cut-before={bar.continuesBefore}
            class:cut-after={bar.continuesAfter}
            class:selected={calendar.selected?.event.id === bar.event.id}
            style:--cal={calendar.color(bar.event)}
            style:grid-column={`${bar.col + 1} / span ${bar.span}`}
            style:grid-row={bar.lane + 1}
            class:draft={!!bar.event.draft_id}
            onclick={(e) => calendar.select(bar.event, e.currentTarget)}
          >
            <span class="title">{bar.event.title || "(No title)"}</span>
          </button>
        {/each}
      </div>
    </div>
  {/if}

  <div class="scroll" bind:this={scroller}>
    <div class="body">
      <div class="ruler" aria-hidden="true">
        {#each hours as h}
          <div class="hour">
            <!-- Not under the current time's own label. -->
            {#if h > 0 && (todayCol < 0 || Math.abs(now - h * 60) > 14)}<span>{hourLabel(h, calendar.hour12)}</span>{/if}
          </div>
        {/each}
        {#if todayCol >= 0}
          <span class="now-label" style:top={pct(now)}>{timeLabel(calendar.now, calendar.hour12)}</span>
        {/if}
      </div>
      {#each days as day, i (day.getTime())}
        <div
          class="col"
          class:today={i === todayCol}
          class:weekend={day.getDay() === 0 || day.getDay() === 6}
          role="presentation"
          onpointerdown={(e) => columnDown(e, i)}
          onpointermove={columnMove}
          onpointerup={columnUp}
          onpointercancel={() => (making = null)}
        >
          {#each placed[i] as p (p.event.id)}
            {@const short = p.height < 40}
            {@const moving = drag?.event.id === p.event.id && landing}
            <button
              class="event block"
              class:short
              class:draft={!!p.event.draft_id}
              class:dragging={!!moving}
              class:editable={p.event.can_edit && !p.event.draft_id}
              class:past={past(p.event)}
              class:free={!p.event.busy}
              class:tentative={p.event.tentative}
              class:declined={declined(p.event)}
              class:cut-before={p.continuesBefore}
              class:cut-after={p.continuesAfter}
              class:selected={calendar.selected?.event.id === p.event.id}
              style:--cal={calendar.color(p.event)}
              style:top={pct(moving ? minutesOf(moving.start) : p.top)}
              style:height={pct(
                Math.max(moving ? (moving.end.getTime() - moving.start.getTime()) / 60000 : p.height, MIN_MINUTES),
              )}
              style:left={`calc(${(p.col / p.cols) * 100}% + 1px)`}
              style:width={`calc(${(p.span / p.cols) * 100}% - 3px)`}
              style:transform={moving ? `translateX(${moving.shift * 100}%)` : undefined}
              onpointerdown={(e) => eventDown(e, p.event)}
              onpointermove={eventMove}
              onpointerup={eventUp}
              onpointercancel={() => (drag = null)}
              onclick={(e) => eventClick(e, p.event)}
            >
              <span class="title">{p.event.title || "(No title)"}</span>
              <span class="time">
                {timeLabel(moving ? moving.start : bounds(p.event).start, calendar.hour12)}{#if !short && p.event.location}<span
                    class="where"
                  >
                    · {p.event.location}</span
                  >{/if}
              </span>
              {#if p.event.can_edit && !p.event.draft_id && !p.continuesAfter}
                <span class="grip" aria-hidden="true"></span>
              {/if}
            </button>
          {/each}
          {#if pressed && making?.day === i}
            <div
              class="event block ghost"
              bind:this={makingEl}
              style:top={pct(minutesOf(pressed.start))}
              style:height={pct((pressed.end.getTime() - pressed.start.getTime()) / 60000)}
            >
              <span class="time">{timeLabel(pressed.start, calendar.hour12)} – {timeLabel(pressed.end, calendar.hour12)}</span>
            </div>
          {:else if pending?.day === i}
            <div
              class="event block ghost"
              style:top={pct(minutesOf(pending.start))}
              style:height={pct(Math.max((pending.end.getTime() - pending.start.getTime()) / 60000, MIN_MINUTES))}
            >
              <span class="title">{pending.title || "New event"}</span>
              <span class="time">{timeLabel(pending.start, calendar.hour12)} – {timeLabel(pending.end, calendar.hour12)}</span>
            </div>
          {/if}
          {#if i === todayCol}
            <div class="now" style:top={pct(now)} aria-hidden="true"></div>
          {/if}
        </div>
      {/each}
    </div>
  </div>
</div>

<style>
  .grid {
    --gutter: 3.6rem;
    --hour: 3rem;
    --lane: 1.45rem;
    --line: color-mix(in srgb, var(--border) 70%, transparent);
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .row,
  .body {
    display: grid;
    grid-template-columns: var(--gutter) repeat(var(--days), minmax(0, 1fr));
  }

  .row {
    padding-right: var(--scrollbar);
  }

  .head {
    border-bottom: 1px solid var(--line);
  }

  .corner {
    display: flex;
    align-items: flex-end;
    justify-content: flex-end;
    padding: 0 var(--space-3) var(--space-3) 0;
    font-size: var(--text-3xs);
    color: var(--fg-muted);
    white-space: nowrap;
  }

  .day-head {
    display: flex;
    align-items: baseline;
    justify-content: center;
    gap: var(--space-3);
    padding: var(--space-4) 0;
    border: none;
    background: none;
    color: var(--fg-muted);
    font: inherit;
    cursor: pointer;
    min-width: 0;
  }

  .dow {
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .num {
    font-size: var(--text-2xl);
    font-weight: var(--weight-medium);
    color: var(--fg);
    min-width: 1.9rem;
    height: 1.9rem;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius-pill);
    transition: background var(--transition-fast);
  }

  .day-head:hover .num {
    background: var(--hover);
  }

  .day-head.today .dow {
    color: var(--accent);
    font-weight: var(--weight-semibold);
  }

  .day-head.today .num {
    background: var(--accent);
    color: var(--on-accent);
    font-weight: var(--weight-semibold);
  }

  .allday {
    border-bottom: 1px solid var(--line);
  }

  .strip-label {
    align-items: center;
    padding-bottom: 0;
    font-size: var(--text-3xs);
  }

  .fold {
    border: none;
    background: var(--hover);
    color: var(--fg-muted);
    font: inherit;
    font-size: var(--text-3xs);
    padding: 0.1rem 0.35rem;
    border-radius: var(--radius-pill);
    cursor: pointer;
  }

  .fold:hover {
    color: var(--fg);
  }

  .bars {
    grid-column: 2 / -1;
    display: grid;
    grid-template-columns: repeat(var(--days), minmax(0, 1fr));
    grid-template-rows: repeat(var(--lanes), var(--lane));
    row-gap: 2px;
    padding: 3px 0;
  }

  .bar-col {
    grid-row: 1 / -1;
    border-left: 1px solid var(--line);
    margin: -3px 0;
  }

  .bar-col.today {
    background: color-mix(in srgb, var(--accent) 4%, transparent);
  }

  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    position: relative;
  }

  .body {
    position: relative;
  }

  .ruler {
    position: relative;
  }

  .hour {
    height: var(--hour);
    position: relative;
  }

  .hour span {
    position: absolute;
    top: 0;
    right: var(--space-3);
    transform: translateY(-50%);
    font-size: var(--text-3xs);
    color: var(--fg-muted);
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }

  .now-label {
    position: absolute;
    right: var(--space-2);
    transform: translateY(-50%);
    padding: 0.05rem 0.3rem;
    border-radius: var(--radius-pill);
    background: var(--calendar-now);
    color: var(--on-danger);
    font-size: var(--text-3xs);
    font-weight: var(--weight-semibold);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
    z-index: 2;
  }

  .col {
    position: relative;
    border-left: 1px solid var(--line);
    /* The hour lines, and a fainter one at each half hour. */
    background-image:
      linear-gradient(to bottom, var(--line) 1px, transparent 1px),
      linear-gradient(
        to bottom,
        transparent calc(var(--hour) / 2),
        color-mix(in srgb, var(--line) 45%, transparent) calc(var(--hour) / 2),
        color-mix(in srgb, var(--line) 45%, transparent) calc(var(--hour) / 2 + 1px),
        transparent calc(var(--hour) / 2 + 1px)
      );
    background-size: 100% var(--hour);
  }

  .col.weekend {
    background-color: color-mix(in srgb, var(--panel-bg) 55%, transparent);
  }

  .col.today {
    background-color: color-mix(in srgb, var(--accent) 4%, transparent);
  }

  .now {
    position: absolute;
    left: -1px;
    right: 0;
    height: 2px;
    margin-top: -1px;
    background: var(--calendar-now);
    z-index: 3;
    pointer-events: none;
  }

  .now::before {
    content: "";
    position: absolute;
    left: -5px;
    top: -4px;
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--calendar-now);
  }

  /* ---------------------------------------------------------- an event */

  .event {
    --c: var(--cal, var(--accent));
    position: relative;
    display: flex;
    min-width: 0;
    overflow: hidden;
    border: none;
    border-radius: var(--radius-sm);
    background: color-mix(in srgb, var(--c) 20%, var(--surface));
    box-shadow: inset 3px 0 0 var(--c);
    color: var(--fg);
    font: inherit;
    text-align: left;
    cursor: pointer;
    transition:
      background var(--transition-fast),
      box-shadow var(--transition-fast);
  }

  .event:hover {
    background: color-mix(in srgb, var(--c) 30%, var(--surface));
  }

  .event:focus-visible {
    outline: var(--focus-outline);
    outline-offset: 1px;
  }

  .event.selected {
    background: color-mix(in srgb, var(--c) 42%, var(--surface));
    box-shadow:
      inset 3px 0 0 var(--c),
      0 0 0 1.5px var(--c),
      var(--shadow-sm);
    z-index: 4;
  }

  .event.past:not(.selected) {
    opacity: 0.55;
  }

  /* Marked free: an outline, so it reads as not taking the time. */
  .event.free:not(.selected) {
    background: color-mix(in srgb, var(--c) 6%, var(--surface));
    box-shadow:
      inset 3px 0 0 var(--c),
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

  .bar {
    align-items: center;
    padding: 0 var(--space-3) 0 calc(var(--space-3) + 3px);
    margin: 0 2px;
    font-size: var(--text-xs);
  }

  .bar.cut-before {
    border-top-left-radius: 0;
    border-bottom-left-radius: 0;
    margin-left: 0;
  }

  .bar.cut-after {
    border-top-right-radius: 0;
    border-bottom-right-radius: 0;
    margin-right: 0;
  }

  .block {
    position: absolute;
    flex-direction: column;
    gap: 1px;
    padding: 0.2rem var(--space-3) 0.2rem calc(var(--space-3) + 3px);
    font-size: var(--text-xs);
    line-height: var(--leading-tight);
    /* A thin gap between stacked ones, so two back to back read as two. */
    border-bottom: 1px solid var(--surface);
  }

  .block.short {
    flex-direction: row;
    align-items: center;
    gap: var(--space-3);
    padding-top: 0;
    padding-bottom: 0;
  }

  .block.cut-before {
    border-top-left-radius: 0;
    border-top-right-radius: 0;
  }

  .block.cut-after {
    border-bottom-left-radius: 0;
    border-bottom-right-radius: 0;
  }

  .bar-col {
    cursor: copy;
  }

  .col {
    touch-action: pan-y;
    cursor: copy;
  }

  .event.editable {
    cursor: grab;
  }

  .event.dragging {
    cursor: grabbing;
    z-index: 5;
    box-shadow:
      inset 3px 0 0 var(--c),
      var(--shadow-popover);
    opacity: 0.92;
  }

  .grip {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    height: 6px;
    cursor: ns-resize;
  }

  /* A Calendar event not yet made: the one being pressed out, or written. */
  .ghost {
    --c: var(--cal, var(--accent));
    pointer-events: none;
    left: 1px;
    width: calc(100% - 3px);
    background: color-mix(in srgb, var(--c) 30%, var(--surface));
    box-shadow:
      inset 3px 0 0 var(--c),
      var(--shadow-sm);
    z-index: 4;
  }

  /* An Agent's draft: dashed, as something not yet in the calendar. */
  .event.draft:not(.selected) {
    background: repeating-linear-gradient(
      -45deg,
      color-mix(in srgb, var(--c) 10%, var(--surface)) 0 6px,
      color-mix(in srgb, var(--c) 4%, var(--surface)) 6px 12px
    );
    box-shadow: inset 0 0 0 1.5px color-mix(in srgb, var(--c) 70%, transparent);
    outline: 1.5px dashed var(--c);
    outline-offset: -1.5px;
  }

  .title {
    font-weight: var(--weight-semibold);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  .block:not(.short) .title {
    flex: none;
    white-space: normal;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
  }

  .time {
    color: color-mix(in srgb, currentColor 72%, transparent);
    font-size: var(--text-2xs);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-variant-numeric: tabular-nums;
    flex: none;
  }

  .short .time {
    flex: 0 1 auto;
  }
</style>
