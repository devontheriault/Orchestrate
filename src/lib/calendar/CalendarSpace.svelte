<script lang="ts">
  /**
   * The Calendar Space (ADR 0017): the user's calendars, read from Google or
   * any CalDAV server by the Host, in a day, week or month. It fills whatever
   * box it's given: a side panel with a small month and the calendars, and
   * the view beside it. The side panel goes when the box is narrow. A phone
   * reads its week as a list and its month as dots over the day picked, with
   * the calendars in the accounts dialog.
   *
   * Keys: T today, D/W/M the view, ←/→ (or J/K) through time, C a new
   * Calendar event, Esc closes.
   */
  import { onDestroy, onMount } from "svelte";
  import { space } from "$lib/spaces/space.svelte";
  import { viewport } from "$lib/layout/viewport.svelte";
  import { panes, MIN_SIDE } from "$lib/layout/panes.svelte";
  import PaneDivider from "$lib/layout/PaneDivider.svelte";
  import { calendar } from "./calendar.svelte";
  import { keyAction, viewDays, viewTitle, shortViewTitle, VIEWS, ago } from "./layout";
  import TimeGrid from "./TimeGrid.svelte";
  import MonthGrid from "./MonthGrid.svelte";
  import MonthCompact from "./MonthCompact.svelte";
  import Agenda from "./Agenda.svelte";
  import CalendarList from "./CalendarList.svelte";
  import MiniMonth from "./MiniMonth.svelte";
  import EventPopover from "./EventPopover.svelte";
  import CalendarAccounts from "./CalendarAccounts.svelte";
  import EventEditor from "./EventEditor.svelte";
  import ScopeChoice from "./ScopeChoice.svelte";
  import { sameDay } from "./layout";

  onMount(() => calendar.start());
  onDestroy(() => calendar.stop());

  let width = $state(1200);
  /** The narrowest a week stays readable at, which the side panel leaves it. */
  const MIN_VIEW = 512;
  /** Too narrow for the side panel beside a readable week. */
  const narrow = $derived(width < 760);
  /** The sidebar's size, unrounded, so the title bar's lead segment meets its edge exactly. */
  let sideBox = $state<readonly ResizeObserverSize[]>();

  // The title bar tops the side panel, while there is one.
  $effect(() => {
    panes.lead.calendar = narrow ? 0 : (sideBox?.[0]?.inlineSize ?? 0);
  });

  const days = $derived(
    calendar.started ? viewDays(calendar.view, calendar.cursor, calendar.weekStart) : [],
  );
  const phone = $derived(viewport.phone);
  const title = $derived(
    !calendar.started
      ? ""
      : phone
        ? shortViewTitle(calendar.view, calendar.cursor, calendar.weekStart, calendar.now)
        : viewTitle(calendar.view, calendar.cursor, calendar.weekStart),
  );
  const accounts = $derived(calendar.overview?.accounts ?? []);
  const loaded = $derived(calendar.overview !== null);
  const trouble = $derived(
    accounts.find((a) => a.status.state === "reconnect" || a.status.state === "error"),
  );
  const lastSync = $derived(
    accounts
      .map((a) => (a.status.state === "ok" ? new Date(a.status.at) : null))
      .filter((d): d is Date => d !== null)
      .sort((a, b) => a.getTime() - b.getTime())[0],
  );
  const syncingNow = $derived(
    calendar.syncing || accounts.some((a) => a.status.state === "syncing"),
  );

  const viewNames = { day: "Day", week: "Week", month: "Month" } as const;

  /**
   * A new Calendar event from the toolbar or C: the next half hour today,
   * or 9:00 on the day in view, or picked under a phone's month.
   */
  function newEvent(anchor: DOMRect | null) {
    const now = new Date();
    const onDay = calendar.view === "day" || (phone && calendar.view === "month");
    const base = sameDay(calendar.cursor, now) || !onDay ? now : calendar.cursor;
    const start = sameDay(base, now)
      ? new Date(now.getFullYear(), now.getMonth(), now.getDate(), now.getHours(), now.getMinutes() < 30 ? 30 : 60)
      : new Date(base.getFullYear(), base.getMonth(), base.getDate(), 9);
    calendar.create(start, new Date(start.getTime() + 30 * 60_000), false, anchor);
  }

  $effect(() => {
    const onKey = (e: KeyboardEvent) => {
      // Only while showing: the rail keeps Calendar mounted behind the other
      // Spaces, hidden with `visibility`, which `offsetParent` doesn't notice.
      if (calendar.accounts || calendar.editing || calendar.asking || space.current !== "calendar") return;
      const t = e.target as HTMLElement | null;
      if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable)) return;
      const action = keyAction(e);
      if (!action) return;
      e.preventDefault();
      switch (action.do) {
        case "today":
          calendar.today();
          break;
        case "view":
          calendar.setView(action.view);
          break;
        case "step":
          calendar.step(action.dir);
          break;
        case "new":
          newEvent(null);
          break;
        case "close":
          calendar.selected = null;
          break;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });
</script>

{#snippet stepButton(dir: -1 | 1)}
  <button
    class="btn btn-ghost btn-icon"
    aria-label={dir < 0 ? "Previous" : "Next"}
    title={dir < 0 ? "Previous (←)" : "Next (→)"}
    onclick={() => calendar.step(dir)}
  >
    <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true"><path d={dir < 0 ? "M10 3 5 8l5 5" : "m6 3 5 5-5 5"} fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" /></svg>
  </button>
{/snippet}

{#snippet accountsButton()}
  <button
    class="btn btn-ghost btn-icon"
    aria-label={phone ? "Calendars and accounts" : "Calendar accounts"}
    onclick={() => (calendar.accounts = "list")}
  >
    <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true"><circle cx="8" cy="5.5" r="2.6" fill="none" stroke="currentColor" stroke-width="1.5" /><path d="M2.8 13.5c.7-2.4 2.7-3.8 5.2-3.8s4.5 1.4 5.2 3.8" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" /></svg>
  </button>
{/snippet}

{#snippet newButton()}
  {#if calendar.writable.length}
    <button
      class="btn btn-primary new"
      class:btn-icon={phone}
      aria-label="New Calendar event"
      title="New Calendar event (C)"
      onclick={(e) => newEvent(e.currentTarget.getBoundingClientRect())}
    >
      <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true"><path d="M8 3v10M3 8h10" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" /></svg>
      {narrow ? "" : "New"}
    </button>
  {/if}
{/snippet}

{#snippet syncButton()}
  {#if accounts.length}
    <button
      class="btn btn-ghost btn-icon"
      class:spinning={syncingNow}
      aria-label="Sync now"
      title="Sync now"
      disabled={syncingNow}
      onclick={() => calendar.sync()}
    >
      <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true"><path d="M13.5 8a5.5 5.5 0 1 1-1.6-3.9M13.5 2.5v3h-3" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>
    </button>
  {/if}
{/snippet}

{#snippet viewSwitch()}
  <div class="segmented" role="tablist" aria-label="View">
    {#each VIEWS as v}
      <button
        role="tab"
        aria-selected={calendar.view === v}
        class:on={calendar.view === v}
        title={`${viewNames[v]} (${v[0].toUpperCase()})`}
        onclick={() => calendar.setView(v)}
      >
        {viewNames[v]}
      </button>
    {/each}
  </div>
{/snippet}

<div class="space" bind:clientWidth={width}>
  <div class="panes">
    {#if !narrow}
      <aside
        class="side"
        style:flex-basis={panes.basis("calendar")}
        style:min-width="{MIN_SIDE.calendar}px"
        style:max-width="calc(100% - {MIN_VIEW}px)"
        bind:borderBoxSize={sideBox}
      >
        {#if calendar.started}
          <MiniMonth />
        {/if}

        <CalendarList />

        <footer>
          <button class="btn btn-ghost btn-sm accounts-btn" onclick={() => (calendar.accounts = "list")}>
            <svg viewBox="0 0 16 16" width="13" height="13" aria-hidden="true"><path d="M8 3v10M3 8h10" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" /></svg>
            {accounts.length ? "Accounts" : "Add an account"}
          </button>
          {#if accounts.length}
            <span class="sync-note" title={trouble ? (trouble.status as { message: string }).message : ""}>
              {#if syncingNow}
                Syncing…
              {:else if trouble}
                <span class="bad">{trouble.status.state === "reconnect" ? "Sign in again" : "Sync failed"}</span>
              {:else if lastSync}
                Synced {ago(lastSync, calendar.now)}
              {/if}
            </span>
          {/if}
        </footer>
      </aside>
      <PaneDivider
        label="Resize calendar sidebar"
        min={MIN_SIDE.calendar}
        minLast={MIN_VIEW}
        onresize={(w) => panes.setSide("calendar", w)}
        onreset={() => panes.setSide("calendar", null)}
      />
    {/if}

    <section class="main">
      {#if phone}
        <header class="toolbar phone">
          <div class="top">
            <h1>{title}</h1>
            <div class="tools">
              <button class="btn btn-sm" onclick={() => calendar.today()}>Today</button>
              {@render accountsButton()}
              {@render syncButton()}
              {@render newButton()}
            </div>
          </div>
          <div class="bottom">
            {@render stepButton(-1)}
            {@render viewSwitch()}
            {@render stepButton(1)}
          </div>
        </header>
      {:else}
        <header class="toolbar">
          <div class="nav">
            <button class="btn" onclick={() => calendar.today()} title="Today (T)">Today</button>
            <div class="arrows">
              {@render stepButton(-1)}
              {@render stepButton(1)}
            </div>
            <h1>{title}</h1>
          </div>
          <div class="tools">
            {#if narrow}{@render accountsButton()}{/if}
            {@render newButton()}
            {@render syncButton()}
            {@render viewSwitch()}
          </div>
        </header>
      {/if}

      {#if calendar.error}
        <div class="banner" role="alert">
          {calendar.error}
          <button class="btn btn-ghost btn-sm" onclick={() => (calendar.error = null)}>Dismiss</button>
        </div>
      {/if}

      {#if loaded && !accounts.length}
        <div class="empty">
          <div class="empty-card">
            <svg viewBox="0 0 48 48" width="44" height="44" aria-hidden="true">
              <rect x="6" y="9" width="36" height="33" rx="7" fill="none" stroke="currentColor" stroke-width="2.2" />
              <path d="M6 19h36M16 5v8M32 5v8" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" />
              <circle cx="17" cy="28" r="2" fill="currentColor" /><circle cx="24" cy="28" r="2" fill="currentColor" /><circle cx="31" cy="28" r="2" fill="currentColor" />
              <circle cx="17" cy="35" r="2" fill="currentColor" /><circle cx="24" cy="35" r="2" fill="currentColor" />
            </svg>
            <h2>Bring in your calendar</h2>
            <p>
              Your calendars stay with Google, Microsoft or your CalDAV server. You read and write them
              here, and your agents can read them and suggest Calendar events for you to send.
            </p>
            <div class="empty-actions">
              <button class="btn btn-primary btn-lg" onclick={() => (calendar.accounts = "google")}>Connect Google Calendar</button>
              <button class="btn btn-lg" onclick={() => (calendar.accounts = "microsoft")}>Connect Outlook</button>
              <button class="btn btn-lg" onclick={() => (calendar.accounts = "caldav")}>Add a CalDAV account</button>
            </div>
          </div>
        </div>
      {:else if calendar.started}
        {#if calendar.view === "month"}
          {#if phone}<MonthCompact />{:else}<MonthGrid />{/if}
        {:else if calendar.view === "week" && phone}
          <Agenda {days} scrollToday />
        {:else}
          <TimeGrid {days} />
        {/if}
      {/if}
    </section>
  </div>

  {#if calendar.selected && !calendar.editing}
    {#key calendar.selected.event.id}
      <EventPopover selection={calendar.selected} />
    {/key}
  {/if}

  {#if calendar.editing}
    <EventEditor editing={calendar.editing} />
  {/if}

  {#if calendar.asking}
    <ScopeChoice asking={calendar.asking} />
  {/if}

  {#if calendar.accounts}
    <CalendarAccounts />
  {/if}
</div>

<style>
  .space {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    background: var(--surface);
  }

  .panes {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
  }

  /* Its divider is the seam to the right. */
  .side {
    flex: 0 1 15.5rem;
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
    padding: var(--space-5) var(--space-5) var(--space-4);
    background: var(--panel-bg);
    min-height: 0;
  }

  .side footer {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--space-2);
  }

  .accounts-btn {
    margin-left: calc(-1 * var(--space-2));
  }

  .sync-note {
    padding-left: var(--space-3);
    font-size: var(--text-2xs);
    color: var(--fg-muted);
  }

  .bad {
    color: var(--danger-text);
  }

  .main {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: var(--space-3) var(--space-5);
    padding: var(--space-4) var(--pad-x);
    border-bottom: 1px solid var(--border);
  }

  .nav,
  .tools {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-width: 0;
  }

  .arrows {
    display: flex;
  }

  h1 {
    margin: 0 0 0 var(--space-3);
    font-size: var(--text-3xl);
    font-weight: var(--weight-semibold);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .segmented {
    display: inline-flex;
    padding: 2px;
    border-radius: var(--radius-md);
    background: var(--hover);
  }

  .segmented button {
    padding: 0.22rem 0.8rem;
    border: none;
    border-radius: calc(var(--radius-md) - 2px);
    background: none;
    color: var(--fg-muted);
    font: inherit;
    font-size: var(--text-sm);
    cursor: pointer;
    transition:
      background var(--transition-fast),
      color var(--transition-fast);
  }

  .segmented button:hover {
    color: var(--fg);
  }

  .segmented button.on {
    background: var(--surface);
    color: var(--fg);
    font-weight: var(--weight-medium);
    box-shadow: var(--shadow-sm);
  }

  /* A phone's: the date and what to do with it over a full-width view switch
     between the arrows, every target a thumb's size. */
  .toolbar.phone {
    flex-direction: column;
    align-items: stretch;
    flex-wrap: nowrap;
    gap: var(--space-4);
    padding: var(--space-4) var(--pad-x) var(--space-4);
  }

  .phone .top,
  .phone .bottom {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
  }

  .phone h1 {
    flex: 1;
    margin: 0;
    font-size: var(--text-2xl);
  }

  .phone .tools {
    flex: none;
    gap: var(--space-1);
  }

  .phone .tools .btn-sm {
    margin-right: var(--space-2);
  }

  .phone .new {
    margin-left: var(--space-2);
  }

  .phone .segmented {
    flex: 1;
  }

  .phone .segmented button {
    flex: 1;
    padding: 0.4rem 0;
    font-size: var(--text-md);
  }

  .spinning svg {
    animation: spin 0.9s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .banner {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
    padding: var(--space-3) var(--pad-x);
    background: var(--danger-soft-bg);
    border-bottom: 1px solid var(--danger-soft-border);
    font-size: var(--text-sm);
    overflow-wrap: anywhere;
  }

  .empty {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: var(--space-8);
  }

  .empty-card {
    max-width: 26rem;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-4);
    text-align: center;
    color: var(--fg-muted);
  }

  .empty-card svg {
    color: var(--accent);
    margin-bottom: var(--space-3);
  }

  .empty-card h2 {
    margin: 0;
    font-size: var(--text-3xl);
    font-weight: var(--weight-semibold);
    color: var(--fg);
  }

  .empty-card p {
    margin: 0;
    font-size: var(--text-md);
    line-height: var(--leading-normal);
  }

  .empty-actions {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: var(--space-4);
    margin-top: var(--space-4);
  }
</style>
