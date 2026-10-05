<script lang="ts">
  /**
   * Writing a Calendar event: a new one, a change to one, or an Agent's draft
   * on its way to being sent. It opens beside what it was opened from, like
   * the detail does. Saving sends it to the provider, which emails the people
   * on it (ADR 0017: only the user sends).
   *
   * Keys: Ctrl/⌘+Enter saves, Esc closes.
   */
  import { dismissOnMove } from "$lib/menus/menu";
  import { viewport } from "$lib/layout/viewport.svelte";
  import Choice from "./Choice.svelte";
  import { calendar, type Editing } from "./calendar.svelte";
  import {
    fromWallClock,
    lastDay,
    parseGuests,
    repeatChoice,
    repeatLabel,
    ruleFor,
    saveLabel,
    untilOf,
    withAllDay,
    withEnd,
    withStart,
    withUntil,
    type RepeatChoice,
  } from "./edit";
  import { addDays, dayKey, placeBeside } from "./layout";

  let { editing }: { editing: Editing } = $props();

  const draft = $derived(editing.draft);
  const existing = $derived(!!editing.event);
  const start = $derived(fromWallClock(draft.start));
  const account = $derived(calendar.writable.find((c) => c.key === editing.calendar)?.account);

  // ---- when
  const startDate = $derived(draft.start.slice(0, 10));
  const startTime = $derived(draft.all_day ? "" : draft.start.slice(11, 16));
  const endTime = $derived(draft.all_day ? "" : draft.end.slice(11, 16));
  const endDate = $derived(draft.all_day ? lastDay(draft) : draft.end.slice(0, 10));
  /** A timed one past midnight shows its end date too. */
  const spansDays = $derived(endDate !== startDate);

  function set(next: typeof draft) {
    editing.draft = next;
  }

  function onStartDate(v: string) {
    if (!v) return;
    set(withStart(draft, draft.all_day ? v : `${v}T${startTime}`));
  }
  function onStartTime(v: string) {
    if (v) set(withStart(draft, `${startDate}T${v}`));
  }
  function onEndTime(v: string) {
    if (!v) return;
    // An end before the start, on the same day, means the next day.
    let day = endDate;
    if (!spansDays && v <= startTime) day = dayKey(addDays(fromWallClock(startDate), 1));
    set(withEnd(draft, `${day}T${v}`));
  }
  function onEndDate(v: string) {
    if (!v) return;
    set(withEnd(draft, draft.all_day ? dayKey(addDays(fromWallClock(v), 1)) : `${v}T${endTime}`));
  }

  // ---- repeat
  const choice = $derived(repeatChoice(draft.repeat, start));
  const until = $derived(untilOf(draft.repeat));
  const choices: RepeatChoice[] = ["none", "daily", "weekdays", "weekly", "monthly", "yearly"];
  const repeatOptions = $derived(
    [...choices, ...(choice === "custom" ? (["custom"] as const) : [])].map((c) => ({
      value: c,
      label: repeatLabel(c, start),
    })),
  );
  function pickRepeat(c: RepeatChoice) {
    if (c !== choice && c !== "custom") set({ ...draft, repeat: ruleFor(c, start, until) });
  }
  function onUntil(v: string) {
    if (draft.repeat) set({ ...draft, repeat: withUntil(draft.repeat, v || null) });
  }

  // ---- calendar
  const calendarOptions = $derived(
    calendar.writable.map((c) => ({
      value: c.key,
      label: c.name,
      color: c.color,
      note: calendar.writable.some((o) => o.account.id !== c.account.id) ? c.account.name : undefined,
    })),
  );

  // ---- guests
  let guestText = $state("");
  let guestBox: HTMLInputElement | undefined = $state();
  const suggestions = $derived.by(() => {
    const q = guestText.trim().toLowerCase();
    if (q.length < 2) return [];
    const taken = new Set(draft.attendees.map((g) => g.email.toLowerCase()));
    return calendar.people
      .filter((p) => !taken.has(p.email.toLowerCase()))
      .filter((p) => p.email.toLowerCase().includes(q) || p.name?.toLowerCase().includes(q))
      .slice(0, 5);
  });

  function addGuests(text: string) {
    const { guests, rest } = parseGuests(text);
    const taken = new Set(draft.attendees.map((g) => g.email.toLowerCase()));
    const fresh = guests.filter((g) => !taken.has(g.email.toLowerCase()));
    if (fresh.length) set({ ...draft, attendees: [...draft.attendees, ...fresh] });
    guestText = rest;
  }

  function removeGuest(email: string) {
    set({ ...draft, attendees: draft.attendees.filter((g) => g.email !== email) });
  }

  function onGuestKey(e: KeyboardEvent) {
    if (e.key === "Enter" || e.key === "," || e.key === ";" || (e.key === "Tab" && guestText.trim())) {
      if (suggestions.length && !guestText.includes("@")) {
        e.preventDefault();
        set({ ...draft, attendees: [...draft.attendees, suggestions[0]] });
        guestText = "";
        return;
      }
      if (guestText.trim()) {
        e.preventDefault();
        addGuests(guestText);
      }
    } else if (e.key === "Backspace" && !guestText && draft.attendees.length) {
      set({ ...draft, attendees: draft.attendees.slice(0, -1) });
    }
  }

  // ---- placing and closing
  let el: HTMLElement | undefined = $state();
  let size = $state({ width: 380, height: 520 });
  const at = $derived.by(() => {
    const view = { width: viewport.width || 1000, height: viewport.height || 800, top: 48 };
    if (!editing.anchor) {
      return {
        left: Math.max(8, (view.width - size.width) / 2),
        top: Math.max(view.top, (view.height - size.height) / 3),
      };
    }
    return placeBeside(editing.anchor, size, view);
  });

  $effect(() => {
    if (!el) return;
    const target = el;
    const measure = () => (size = { width: target.offsetWidth, height: target.offsetHeight });
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(target);
    return () => ro.disconnect();
  });

  let titleEl: HTMLInputElement | undefined = $state();
  $effect(() => {
    titleEl?.focus();
  });

  // A press outside closes a new, untouched one; anything typed is kept
  // until the user says otherwise, so a stray click loses nothing.
  $effect(() =>
    dismissOnMove(
      () => [el, ...document.querySelectorAll<HTMLElement>(".popover-above, .scope")],
      () => {
        if (!existing && !draft.title.trim() && !editing.draftId) close();
      },
      { scroll: false },
    ),
  );

  function close() {
    calendar.editing = null;
    calendar.editError = null;
  }

  function save(e?: Event) {
    e?.preventDefault();
    if (guestText.trim()) addGuests(guestText);
    const r = (e?.target as HTMLElement | undefined)?.getBoundingClientRect();
    calendar.save(r ? { x: r.left, y: r.top } : { x: at.left + 40, y: at.top + 40 });
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.stopPropagation();
      close();
    } else if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      save();
    }
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<form
  class="popover editor"
  class:sheet={viewport.phone}
  bind:this={el}
  aria-label={existing ? "Change Calendar event" : "New Calendar event"}
  style:left={viewport.phone ? undefined : `${at.left}px`}
  style:top={viewport.phone ? undefined : `${at.top}px`}
  style:--cal={calendar.writable.find((c) => c.key === editing.calendar)?.color}
  onsubmit={save}
  onkeydown={onKey}
>
  {#if editing.draftId}
    <div class="drafted">
      <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true"><path d="M8 1.5l1.6 4.2 4.4.3-3.4 2.8 1.1 4.3L8 10.7l-3.7 2.4 1.1-4.3L2 6l4.4-.3z" fill="currentColor" /></svg>
      <span>
        Drafted by an Agent. Nothing is sent until you save it.
        {#if editing.note}<em class="why">“{editing.note}”</em>{/if}
      </span>
    </div>
  {/if}

  <div class="title-row">
    <span class="swatch" aria-hidden="true"></span>
    <input
      bind:this={titleEl}
      class="title"
      placeholder={existing ? "Title" : "New event"}
      aria-label="Title"
      value={draft.title}
      oninput={(e) => set({ ...draft, title: e.currentTarget.value })}
    />
  </div>

  <div class="row when">
    <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true"><circle cx="8" cy="8" r="6.25" fill="none" stroke="currentColor" stroke-width="1.4" /><path d="M8 4.5V8l2.5 1.5" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" /></svg>
    <div class="fields">
      {#if draft.all_day}
        <div class="times">
          <input class="input" type="date" aria-label="First day" value={startDate} onchange={(e) => onStartDate(e.currentTarget.value)} />
          <span class="dash">–</span>
          <input class="input" type="date" aria-label="Last day" value={endDate} onchange={(e) => onEndDate(e.currentTarget.value)} />
        </div>
      {:else}
        <input class="input" type="date" aria-label="Starts on" value={startDate} onchange={(e) => onStartDate(e.currentTarget.value)} />
        <div class="times">
          <input class="input" type="time" step="900" aria-label="Starts at" value={startTime} onchange={(e) => onStartTime(e.currentTarget.value)} />
          <span class="dash">–</span>
          <input class="input" type="time" step="900" aria-label="Ends at" value={endTime} onchange={(e) => onEndTime(e.currentTarget.value)} />
        </div>
        {#if spansDays}
          <label class="ends-on">
            ends
            <input class="input" type="date" aria-label="Ends on" value={endDate} onchange={(e) => onEndDate(e.currentTarget.value)} />
          </label>
        {/if}
      {/if}
      <div class="toggles">
        <label class="switch">
          <input type="checkbox" role="switch" checked={draft.all_day} onchange={(e) => set(withAllDay(draft, e.currentTarget.checked))} />
          <span class="track"><span class="thumb"></span></span>
          All day
        </label>
        <label class="switch">
          <input type="checkbox" role="switch" checked={!draft.busy} onchange={(e) => set({ ...draft, busy: !e.currentTarget.checked })} />
          <span class="track"><span class="thumb"></span></span>
          Show as free
        </label>
      </div>
    </div>
  </div>

  <div class="row">
    <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true"><path d="M12.5 5.5A5 5 0 0 0 3.6 4M3.5 10.5A5 5 0 0 0 12.4 12" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" /><path d="M3.3 1.8v2.6h2.6M12.7 14.2v-2.6h-2.6" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg>
    <div class="fields repeat">
      <Choice label="Repeat" options={repeatOptions} bind:value={() => choice, pickRepeat} />
      {#if draft.repeat}
        <label class="until">
          until
          <input class="input" type="date" aria-label="Repeats until" value={until ?? ""} min={startDate} onchange={(e) => onUntil(e.currentTarget.value)} />
        </label>
      {/if}
    </div>
  </div>

  <div class="row">
    <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true"><circle cx="6" cy="5.5" r="2.5" fill="none" stroke="currentColor" stroke-width="1.4" /><path d="M1.5 13.5c.5-2.5 2.3-4 4.5-4s4 1.5 4.5 4" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" /><path d="M12.5 5v5M10 7.5h5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" /></svg>
    <div class="fields">
      <div class="guests">
        {#each draft.attendees as g (g.email)}
          <span class="chip" title={g.email}>
            {g.name || g.email}
            <button type="button" aria-label={`Remove ${g.email}`} onclick={() => removeGuest(g.email)}>×</button>
          </span>
        {/each}
        <input
          bind:this={guestBox}
          class="guest-input"
          placeholder={draft.attendees.length ? "Add more" : "Invite people by email"}
          aria-label="Invite people"
          bind:value={guestText}
          onkeydown={onGuestKey}
          onblur={() => guestText.includes("@") && addGuests(guestText)}
          onpaste={(e) => {
            const text = e.clipboardData?.getData("text") ?? "";
            if (parseGuests(text).guests.length) {
              e.preventDefault();
              addGuests(guestText + " " + text);
            }
          }}
        />
      </div>
      {#if suggestions.length}
        <ul class="suggest">
          {#each suggestions as p (p.email)}
            <li>
              <button
                type="button"
                onclick={() => {
                  set({ ...draft, attendees: [...draft.attendees, p] });
                  guestText = "";
                  guestBox?.focus();
                }}
              >
                <span>{p.name ?? p.email}</span>{#if p.name}<span class="muted">{p.email}</span>{/if}
              </button>
            </li>
          {/each}
        </ul>
      {/if}
      {#if draft.attendees.length && account && !account.invites}
        <p class="hint warn">This server doesn't email invitations: they'll see it only if their own calendar reads this one.</p>
      {:else if draft.attendees.length}
        <p class="hint">{account?.kind === "google" ? "Google" : account?.kind === "microsoft" ? "Outlook" : "Your server"} emails them an invitation when you save.</p>
      {/if}
    </div>
  </div>

  <div class="row">
    <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true"><path d="M8 14.5s4.5-4.3 4.5-8a4.5 4.5 0 0 0-9 0c0 3.7 4.5 8 4.5 8z" fill="none" stroke="currentColor" stroke-width="1.4" /><circle cx="8" cy="6.5" r="1.6" fill="currentColor" /></svg>
    <input class="input plain" placeholder="Add a place or a link" aria-label="Location" value={draft.location ?? ""} oninput={(e) => set({ ...draft, location: e.currentTarget.value || null })} />
  </div>

  <div class="row">
    <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true"><path d="M3 4h10M3 8h10M3 12h6" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" /></svg>
    <textarea class="textarea plain" rows="3" placeholder="Notes" aria-label="Notes" value={draft.description ?? ""} oninput={(e) => set({ ...draft, description: e.currentTarget.value || null })}></textarea>
  </div>

  <div class="row">
    <span class="cal-dot" aria-hidden="true"></span>
    <div class="fields">
      <Choice
        label="Calendar"
        options={calendarOptions}
        bind:value={() => editing.calendar, (k) => (editing.calendar = k)}
      />
    </div>
  </div>

  {#if calendar.editError}<p class="field-error">{calendar.editError}</p>{/if}

  <footer>
    {#if editing.event?.can_edit || editing.draftId}
      <button
        type="button"
        class="btn btn-ghost btn-sm danger"
        disabled={calendar.saving}
        onclick={(e) => {
          const r = e.currentTarget.getBoundingClientRect();
          const target = editing.event ?? calendar.events.find((x) => x.draft_id === editing.draftId);
          if (target) calendar.remove(target, { x: r.left, y: r.top });
          if (editing.draftId) close();
        }}
      >
        {editing.draftId ? "Discard" : "Delete"}
      </button>
    {/if}
    <span class="spacer"></span>
    <button type="button" class="btn btn-ghost" onclick={close}>Cancel</button>
    <button class="btn btn-primary" disabled={calendar.saving || !draft.title.trim()}>
      {calendar.saving ? "Saving…" : saveLabel(draft.attendees.length, existing)}
    </button>
  </footer>
</form>

<style>
  .editor {
    --c: var(--cal, var(--accent));
    width: min(25rem, calc(100vw - 16px));
    max-height: calc(100vh - 60px);
    overflow-y: auto;
    padding: var(--space-5) var(--space-6) var(--space-5);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    font-size: var(--text-sm);
  }

  .editor.sheet {
    left: 0;
    right: 0;
    bottom: 0;
    width: auto;
    max-height: 85vh;
    border-radius: var(--radius-xl) var(--radius-xl) 0 0;
    padding-bottom: calc(var(--space-6) + var(--safe-bottom));
  }

  .drafted {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    border-radius: var(--radius-md);
    background: var(--warning-soft-bg);
    border: 1px solid var(--warning-soft-border);
    color: var(--fg);
    font-size: var(--text-xs);
  }

  .why {
    display: block;
    margin-top: var(--space-2);
  }

  .drafted svg {
    color: var(--warning-text);
    flex: none;
  }

  .title-row {
    display: flex;
    align-items: center;
    gap: var(--space-4);
  }

  .swatch {
    flex: none;
    width: 0.8rem;
    height: 0.8rem;
    border-radius: var(--radius-xs);
    background: var(--c);
  }

  .title {
    flex: 1;
    min-width: 0;
    border: none;
    border-bottom: 1px solid transparent;
    background: transparent;
    color: var(--fg);
    font: inherit;
    font-size: var(--text-2xl);
    font-weight: var(--weight-semibold);
    padding: 0.15rem 0;
  }

  .title:focus {
    outline: none;
    border-bottom-color: var(--accent);
  }

  .title::placeholder {
    color: var(--fg-muted);
    font-weight: var(--weight-medium);
  }

  .row {
    display: flex;
    align-items: flex-start;
    gap: var(--space-4);
  }

  .row > svg,
  .cal-dot {
    flex: none;
    width: 0.8rem;
    margin-top: 0.5rem;
    color: var(--fg-muted);
  }

  .cal-dot {
    height: 0.8rem;
    border-radius: 50%;
    background: var(--c);
    margin-top: 0.45rem;
  }

  .fields {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .times {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto minmax(0, 1fr);
    align-items: center;
    gap: var(--space-3);
  }

  .ends-on {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    align-items: center;
    gap: var(--space-3);
    color: var(--fg-muted);
    font-size: var(--text-xs);
  }

  .dash {
    color: var(--fg-muted);
  }

  .toggles {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3) var(--space-6);
    font-size: var(--text-xs);
    color: var(--fg-muted);
  }

  .repeat {
    flex-direction: row;
    align-items: center;
    flex-wrap: wrap;
  }

  .repeat > :global(.trigger) {
    flex: 1 1 12rem;
  }

  .until {
    display: inline-flex;
    align-items: center;
    gap: var(--space-3);
    color: var(--fg-muted);
    font-size: var(--text-xs);
  }

  .guests {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
    padding: 0.2rem var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--control-radius);
    background: var(--surface);
    min-height: 2rem;
  }

  .guests:focus-within {
    border-color: var(--accent);
    box-shadow: var(--focus-ring);
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    max-width: 100%;
    padding: 0.1rem 0.15rem 0.1rem 0.5rem;
    border-radius: var(--radius-pill);
    background: var(--hover);
    font-size: var(--text-xs);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .chip button {
    border: none;
    background: none;
    color: var(--fg-muted);
    cursor: pointer;
    font: inherit;
    padding: 0 0.3rem;
    border-radius: var(--radius-pill);
  }

  .chip button:hover {
    color: var(--fg);
    background: var(--border);
  }

  .guest-input {
    flex: 1 1 8rem;
    min-width: 6rem;
    border: none;
    background: transparent;
    color: var(--fg);
    font: inherit;
    padding: 0.2rem 0;
  }

  .guest-input:focus {
    outline: none;
  }

  .suggest {
    list-style: none;
    margin: 0;
    padding: var(--space-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
  }

  .suggest button {
    display: flex;
    gap: var(--space-4);
    width: 100%;
    padding: 0.3rem var(--space-3);
    border: none;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--fg);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .suggest button:hover,
  .suggest button:focus-visible {
    background: var(--hover);
  }

  .muted {
    color: var(--fg-muted);
  }

  .hint {
    margin: 0;
    font-size: var(--text-2xs);
    color: var(--fg-muted);
  }

  .hint.warn {
    color: var(--warning-text);
  }

  .plain {
    flex: 1;
  }

  .field-error {
    margin: 0;
  }

  footer {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding-top: var(--space-4);
    border-top: 1px solid var(--border);
  }

  .spacer {
    flex: 1;
  }

  .danger {
    color: var(--danger-text);
  }
</style>
