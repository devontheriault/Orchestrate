<script lang="ts">
  /**
   * A Calendar event's detail, beside the box it was opened from: when, where,
   * who, the notes, and the meeting to join; and from here, changing it,
   * deleting it, or answering its invitation. An Agent's draft says so, and
   * offers to send it or throw it away.
   */
  import { openUrl } from "@tauri-apps/plugin-opener";
  import type { Attendee, CalendarAnswer } from "$lib/api";
  import LinkedText from "$lib/markdown/LinkedText.svelte";
  import { safeHref } from "$lib/markdown/markdown";
  import { dismissOnMove } from "$lib/menus/menu";
  import { viewport } from "$lib/layout/viewport.svelte";
  import { calendar, type Selection } from "./calendar.svelte";
  import { calendarKey, meetingName, placeBeside, whenLabel, zoneNote } from "./layout";

  let { selection }: { selection: Selection } = $props();

  const event = $derived(selection.event);
  const cal = $derived(calendar.calendars.get(calendarKey(event.account_id, event.calendar_id)));
  const meeting = $derived(safeHref(event.meeting_url));
  const zone = $derived(
    zoneNote(event, Intl.DateTimeFormat().resolvedOptions().timeZone, calendar.hour12),
  );
  const link = $derived(safeHref(event.link));
  /** People, with the organizer first and the user next. */
  const people = $derived(
    [...event.attendees].sort(
      (a, b) => Number(b.organizer) - Number(a.organizer) || Number(b.self) - Number(a.self),
    ),
  );
  let showAll = $state(false);
  const PEOPLE_SHOWN = 6;

  let el: HTMLElement | undefined = $state();
  let size = $state({ width: 340, height: 300 });
  const at = $derived(
    placeBeside(selection.anchor, size, {
      width: viewport.width || 1000,
      height: viewport.height || 800,
      top: document.querySelector("header.chrome")?.getBoundingClientRect().bottom ?? 0,
    }),
  );

  $effect(() => {
    if (!el) return;
    const target = el;
    const measure = () => (size = { width: target.offsetWidth, height: target.offsetHeight });
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(target);
    return () => ro.disconnect();
  });

  $effect(() => dismissOnMove(() => [el], close));

  $effect(() => {
    el?.focus();
  });

  function close() {
    calendar.selected = null;
  }

  function open(url: string) {
    openUrl(url).catch((err) => console.error("couldn't open link", url, err));
  }

  function response(a: Attendee): { mark: string; label: string } {
    switch (a.response) {
      case "accepted":
        return { mark: "✓", label: "Going" };
      case "declined":
        return { mark: "✕", label: "Not going" };
      case "tentative":
        return { mark: "?", label: "Maybe" };
      default:
        return { mark: "", label: "Hasn't answered" };
    }
  }

  function initials(a: Attendee): string {
    const name = (a.name || a.email).replace(/@.*/, "");
    const parts = name.split(/[\s._-]+/).filter(Boolean);
    return ((parts[0]?.[0] ?? "") + (parts[1]?.[0] ?? "")).toUpperCase() || "?";
  }

  const going = $derived(event.attendees.filter((a) => a.response === "accepted").length);

  /** The user, when they're invited rather than organizing: they can answer. */
  const me = $derived(
    event.draft_id ? undefined : event.attendees.find((a) => a.self && !a.organizer && !event.organizer?.self),
  );
  const answers: { value: CalendarAnswer; label: string }[] = [
    { value: "accepted", label: "Yes" },
    { value: "tentative", label: "Maybe" },
    { value: "declined", label: "No" },
  ];

  function where(e: MouseEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    return { x: r.left, y: r.bottom + 6 };
  }

  function edit() {
    calendar.edit(event, el?.getBoundingClientRect() ?? selection.anchor);
  }
</script>

<div
  class="popover detail"
  class:sheet={viewport.phone}
  bind:this={el}
  role="dialog"
  aria-label={event.title || "Calendar event"}
  tabindex="-1"
  style:--cal={cal?.color}
  style:left={viewport.phone ? undefined : `${at.left}px`}
  style:top={viewport.phone ? undefined : `${at.top}px`}
  onkeydown={(e) => {
    if (e.key === "Escape") {
      e.stopPropagation();
      close();
    }
  }}
>
  <header>
    <span class="swatch" aria-hidden="true"></span>
    <h2>{event.title || "(No title)"}</h2>
    {#if event.can_edit && !event.draft_id}
      <button class="btn btn-ghost btn-icon tool" aria-label="Change" title="Change" onclick={edit}>
        <svg viewBox="0 0 16 16" width="13" height="13" aria-hidden="true"><path d="M10.5 2.5l3 3-8 8H2.5v-3z" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round" /></svg>
      </button>
      <button class="btn btn-ghost btn-icon tool" aria-label="Delete" title="Delete" onclick={(e) => calendar.remove(event, where(e))}>
        <svg viewBox="0 0 16 16" width="13" height="13" aria-hidden="true"><path d="M2.5 4.5h11M6 4.5V3h4v1.5M4 4.5l.7 9h6.6l.7-9" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg>
      </button>
    {/if}
    <button class="btn btn-ghost btn-icon close" aria-label="Close" onclick={close}>
      <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
        <path d="M4 4l8 8M12 4l-8 8" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" />
      </svg>
    </button>
  </header>

  <div class="when">
    <span>{whenLabel(event, calendar.hour12)}</span>
    {#if event.recurring || zone || event.tentative || (!event.busy && !event.draft_id)}
      <span class="tags">
        {#if event.recurring}<span class="badge">Repeats</span>{/if}
        {#if event.tentative}<span class="badge">Tentative</span>{/if}
        {#if !event.busy && !event.draft_id}<span class="badge">Free</span>{/if}
        {#if zone}<span class="badge" title={`Made in ${event.time_zone}`}>{zone}</span>{/if}
      </span>
    {/if}
  </div>

  {#if event.draft_id}
    <div class="drafted">
      <strong>Drafted by an Agent.</strong>
      Nothing is saved or sent until you do.
      {#if event.draft_note}<p class="why">“{event.draft_note}”</p>{/if}
    </div>
  {/if}

  {#if me}
    <div class="rsvp" role="group" aria-label="Going?">
      <span>Going?</span>
      {#each answers as a (a.value)}
        <button
          class="btn btn-sm"
          class:on={me.response === a.value}
          disabled={calendar.saving}
          aria-pressed={me.response === a.value}
          onclick={(e) => calendar.respond(event, a.value, where(e))}
        >
          {a.label}
        </button>
      {/each}
    </div>
  {/if}

  {#if meeting}
    <button class="btn btn-primary join" onclick={() => open(meeting)}>
      <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
        <rect x="1.5" y="4" width="9" height="8" rx="2" fill="none" stroke="currentColor" stroke-width="1.5" />
        <path d="M10.5 7l4-2.5v7l-4-2.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round" />
      </svg>
      Join {meetingName(meeting)}
    </button>
  {/if}

  {#if event.location}
    <div class="field">
      <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
        <path d="M8 14.5s4.5-4.3 4.5-8a4.5 4.5 0 0 0-9 0c0 3.7 4.5 8 4.5 8z" fill="none" stroke="currentColor" stroke-width="1.4" />
        <circle cx="8" cy="6.5" r="1.6" fill="currentColor" />
      </svg>
      <span class="text"><LinkedText text={event.location} /></span>
    </div>
  {/if}

  {#if people.length}
    <div class="field people">
      <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
        <circle cx="6" cy="5.5" r="2.5" fill="none" stroke="currentColor" stroke-width="1.4" />
        <path d="M1.5 13.5c.5-2.5 2.3-4 4.5-4s4 1.5 4.5 4" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
        <path d="M11 3.2a2.5 2.5 0 0 1 0 4.6M12.5 9.8c1 .6 1.8 1.9 2 3.7" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
      </svg>
      <div class="text">
        <div class="count">
          {people.length} {people.length === 1 ? "person" : "people"}{#if going}<span class="muted"> · {going} going</span>{/if}
        </div>
        <ul>
          {#each showAll ? people : people.slice(0, PEOPLE_SHOWN) as a (a.email)}
            {@const r = response(a)}
            <li title={a.email}>
              <span class="avatar" class:declined={a.response === "declined"}>
                {initials(a)}
                {#if r.mark}<span class="answer" data-r={a.response} title={r.label}>{r.mark}</span>{/if}
              </span>
              <span class="who">
                <span class="name">{a.name || a.email}{#if a.self}<span class="muted"> (you)</span>{/if}</span>
                {#if a.organizer}<span class="muted role">Organizer</span>{/if}
              </span>
            </li>
          {/each}
        </ul>
        {#if people.length > PEOPLE_SHOWN}
          <button class="more" onclick={() => (showAll = !showAll)}>
            {showAll ? "Show fewer" : `Show all ${people.length}`}
          </button>
        {/if}
      </div>
    </div>
  {/if}

  {#if event.description}
    <div class="field notes">
      <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
        <path d="M3 4h10M3 8h10M3 12h6" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
      </svg>
      <div class="text description"><LinkedText text={event.description} /></div>
    </div>
  {/if}

  <footer>
    <span class="cal">
      <span class="dot" aria-hidden="true"></span>
      {cal?.name ?? "Calendar"}
    </span>
    {#if event.draft_id}
      <span class="actions">
        <button class="btn btn-ghost btn-sm" onclick={(e) => calendar.remove(event, where(e))}>Discard</button>
        <button class="btn btn-primary btn-sm" onclick={edit}>Review and send</button>
      </span>
    {:else if link}
      <button class="btn btn-ghost btn-sm" onclick={() => open(link)}>
        Open in {cal?.kind === "google" ? "Google Calendar" : cal?.kind === "microsoft" ? "Outlook" : "browser"}
      </button>
    {/if}
  </footer>
</div>

<style>
  .detail {
    --c: var(--cal, var(--accent));
    width: min(23rem, calc(100vw - 16px));
    max-height: calc(100vh - 60px);
    overflow-y: auto;
    padding: var(--space-5) var(--space-6) var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    font-size: var(--text-sm);
  }

  .detail.sheet {
    left: 0;
    right: 0;
    bottom: 0;
    width: auto;
    max-height: 75vh;
    border-radius: var(--radius-xl) var(--radius-xl) 0 0;
    padding-bottom: calc(var(--space-6) + var(--safe-bottom));
  }

  header {
    display: flex;
    align-items: flex-start;
    gap: var(--space-4);
  }

  .swatch {
    flex: none;
    width: 0.8rem;
    height: 0.8rem;
    margin-top: 0.3rem;
    border-radius: var(--radius-xs);
    background: var(--c);
  }

  h2 {
    flex: 1;
    margin: 0;
    font-size: var(--text-2xl);
    font-weight: var(--weight-semibold);
    line-height: var(--leading-tight);
    overflow-wrap: anywhere;
  }

  .close {
    margin: -0.2rem -0.5rem 0 0;
  }

  .tool {
    margin-top: -0.2rem;
  }

  .drafted {
    padding: var(--space-3) var(--space-4);
    border-radius: var(--radius-md);
    background: var(--warning-soft-bg);
    border: 1px solid var(--warning-soft-border);
    font-size: var(--text-xs);
  }

  .why {
    margin: var(--space-3) 0 0;
    font-style: italic;
  }

  .rsvp {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding-left: calc(0.8rem + var(--space-4));
    color: var(--fg-muted);
  }

  .rsvp span {
    margin-right: var(--space-2);
  }

  .rsvp .btn.on {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 16%, var(--surface));
    color: var(--fg);
  }

  .actions {
    display: inline-flex;
    gap: var(--space-3);
  }

  .when {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding-left: calc(0.8rem + var(--space-4));
    color: var(--fg);
  }

  .tags {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .join {
    align-self: flex-start;
    margin-left: calc(0.8rem + var(--space-4));
  }

  .field {
    display: flex;
    gap: var(--space-4);
    align-items: flex-start;
    color: var(--fg);
  }

  .field > svg {
    flex: none;
    width: 0.8rem;
    margin-top: 0.15rem;
    color: var(--fg-muted);
  }

  .text {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .description {
    white-space: pre-wrap;
    line-height: var(--leading-normal);
    max-height: 14rem;
    overflow-y: auto;
    color: var(--fg);
  }

  .count {
    margin-bottom: var(--space-3);
  }

  .muted {
    color: var(--fg-muted);
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  li {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    min-width: 0;
  }

  .avatar {
    position: relative;
    flex: none;
    width: 1.6rem;
    height: 1.6rem;
    border-radius: 50%;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: var(--hover);
    color: var(--fg-muted);
    font-size: var(--text-3xs);
    font-weight: var(--weight-semibold);
  }

  .avatar.declined {
    opacity: 0.55;
  }

  .answer {
    position: absolute;
    right: -3px;
    bottom: -3px;
    width: 0.85rem;
    height: 0.85rem;
    border-radius: 50%;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    font-size: 0.5rem;
    border: 1.5px solid var(--float-bg, var(--surface));
    background: var(--stopped);
    color: var(--surface);
  }

  .answer[data-r="accepted"] {
    background: var(--success);
    color: var(--on-success);
  }

  .answer[data-r="declined"] {
    background: var(--danger);
    color: var(--on-danger);
  }

  .answer[data-r="tentative"] {
    background: var(--warning);
    color: var(--on-warning);
  }

  .who {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .role {
    font-size: var(--text-2xs);
  }

  .more {
    margin-top: var(--space-3);
    padding: 0;
    border: none;
    background: none;
    color: var(--accent);
    font: inherit;
    font-size: var(--text-xs);
    cursor: pointer;
  }

  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
    padding-top: var(--space-4);
    border-top: 1px solid var(--border);
    color: var(--fg-muted);
    font-size: var(--text-xs);
  }

  .cal {
    display: inline-flex;
    align-items: center;
    gap: var(--space-3);
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .dot {
    flex: none;
    width: 0.5rem;
    height: 0.5rem;
    border-radius: 50%;
    background: var(--c);
  }
</style>
