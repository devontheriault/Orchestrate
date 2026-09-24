<script lang="ts">
  /**
   * An agent's options — its advisor and its output style — beside the model
   * and mode pickers. Unlike those, what's picked here is the agent's rather
   * than the next prompt's: it sticks for every Turn after, which is the point,
   * since typed as a slash command it would last one.
   *
   * One menu, two groups: each group is its own choice with its own tick, and
   * the keys walk both as one list.
   */
  import type { AgentOptions } from "$lib/api";
  import { ADVISORS, advisorLabel } from "$lib/picks";
  import { dismissOnMove, menuStyle, opensMenu, placeMenu, stepActive, type Placement } from "./menu";

  let {
    options,
    styles,
    onchange,
    onopen,
    disabled = false,
    label = "Options",
  }: {
    options: AgentOptions;
    /** The output styles `claude` offers, by name; "default" among them is left out. */
    styles: string[];
    onchange: (options: AgentOptions) => void;
    /** The menu is coming up: a moment to ask for the latest styles. */
    onopen?: () => void;
    disabled?: boolean;
    label?: string;
  } = $props();

  const uid = $props.id();

  type Row = { key: keyof AgentOptions; id: string | null; name: string };

  const advisor = $derived(options.advisor ?? null);
  const style = $derived(options.output_style ?? null);

  // A value this build or this directory doesn't list still gets a row, so the
  // menu says what the agent runs with rather than nothing at all.
  const advisorRows = $derived<Row[]>([
    { key: "advisor", id: null, name: "Default" },
    ...ADVISORS.map((a) => ({ key: "advisor" as const, id: a.id as string, name: a.name as string })),
    ...(advisor && !ADVISORS.some((a) => a.id === advisor)
      ? [{ key: "advisor" as const, id: advisor, name: advisor }]
      : []),
  ]);
  const styleRows = $derived.by<Row[]>(() => {
    const listed = styles.filter((s) => s !== "default");
    if (style && !listed.includes(style)) listed.push(style);
    return [
      { key: "output_style", id: null, name: "Default" },
      ...listed.map((s) => ({ key: "output_style" as const, id: s, name: s })),
    ];
  });
  const rows = $derived([...advisorRows, ...styleRows]);

  /** The trigger says what's set, so an agent with none reads as plain. */
  const summary = $derived(
    [advisor && `${advisorLabel(advisor)} advisor`, style].filter(Boolean).join(" · ") || "Options",
  );

  function isOn(row: Row): boolean {
    return (options[row.key] ?? null) === row.id;
  }

  let open = $state(false);
  let active = $state(0);
  let keyNav = $state(false);

  let triggerEl: HTMLButtonElement | undefined = $state();
  let listEl: HTMLElement | undefined = $state();
  let placement = $state<Placement | null>(null);

  function openMenu() {
    if (disabled || !triggerEl) return;
    onopen?.();
    active = Math.max(0, rows.findIndex(isOn));
    keyNav = true;
    // Like the pickers beside it, it lives at the foot of the window, so it
    // opens upward; wide enough for a heading over each group.
    placement = placeMenu(triggerEl, { minWidth: 200, maxHeight: 360, align: "right" });
    open = true;
  }

  function close(refocus = true) {
    if (!open) return;
    open = false;
    if (refocus) triggerEl?.focus();
  }

  function pick(row: Row) {
    if (!isOn(row)) onchange({ ...options, [row.key]: row.id });
    close();
  }

  $effect(() => {
    if (!open) return;
    return dismissOnMove(() => [listEl, triggerEl], () => close(false));
  });

  $effect(() => {
    if (open) listEl?.focus();
  });

  $effect(() => {
    if (!open || !keyNav) return;
    active;
    listEl
      ?.querySelector<HTMLElement>('[data-active="true"]')
      ?.scrollIntoView({ block: "nearest" });
  });

  function onTriggerKeydown(e: KeyboardEvent) {
    if (opensMenu(e.key)) {
      e.preventDefault();
      openMenu();
    }
  }

  function onListKeydown(e: KeyboardEvent) {
    keyNav = true;
    const next = stepActive(e.key, active, rows.length);
    if (next !== null) {
      e.preventDefault();
      active = next;
      return;
    }
    switch (e.key) {
      case "Escape":
        e.preventDefault();
        close();
        break;
      case "Tab":
        close(false);
        break;
      case "Enter":
      case " ":
        e.preventDefault();
        pick(rows[active]);
        break;
    }
  }
</script>

<span class="picker">
  <button
    bind:this={triggerEl}
    type="button"
    class="btn btn-select btn-ghost trigger"
    {disabled}
    aria-haspopup="listbox"
    aria-expanded={open}
    aria-label={label}
    title={`${label} — kept for every prompt after`}
    onclick={() => (open ? close() : openMenu())}
    onkeydown={onTriggerKeydown}
  >
    <span class="btn-select-label">{summary}</span>
    <span class="btn-select-chevron" aria-hidden="true">
      <svg viewBox="0 0 10 6" width="10" height="6">
        <path
          d="M1 1l4 4 4-4"
          fill="none"
          stroke="currentColor"
          stroke-width="1.5"
          stroke-linecap="round"
          stroke-linejoin="round"
        />
      </svg>
    </span>
  </button>
</span>

{#snippet row(r: Row, i: number)}
  <!-- The listbox owns the keyboard: the rows aren't focusable, so a key
       handler on each one would never fire. -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    id={`${uid}-opt-${i}`}
    role="option"
    tabindex="-1"
    aria-selected={isOn(r)}
    data-active={i === active}
    class="menu-item"
    class:on={isOn(r)}
    onclick={() => pick(r)}
    onpointermove={() => {
      keyNav = false;
      active = i;
    }}
  >
    <span class="menu-tick" aria-hidden="true">{isOn(r) ? "✓" : ""}</span>
    <span class="menu-label">{r.name}</span>
  </div>
{/snippet}

<!-- Ours rather than a native <select>, and fixed: see menu.ts. -->
{#if open && placement}
  <div
    bind:this={listEl}
    class="popover menu"
    role="listbox"
    aria-label={label}
    aria-activedescendant={`${uid}-opt-${active}`}
    tabindex="-1"
    onkeydown={onListKeydown}
    style={menuStyle(placement)}
  >
    <div class="menu-title" id={`${uid}-advisor`}>Advisor</div>
    <div role="group" aria-labelledby={`${uid}-advisor`}>
      {#each advisorRows as r, i (r.id)}{@render row(r, i)}{/each}
    </div>
    <div class="menu-title" id={`${uid}-style`}>Output style</div>
    <div role="group" aria-labelledby={`${uid}-style`}>
      {#each styleRows as r, i (r.id)}{@render row(r, advisorRows.length + i)}{/each}
    </div>
  </div>
{/if}

<style>
  .picker {
    position: relative;
    display: inline-flex;
    align-items: center;
    min-width: 0;
    max-width: 13rem;
  }

  /* Sized as the composer's other pickers: a quiet secondary control. */
  .trigger {
    width: 100%;
    font-size: var(--text-xs);
    padding: 0.25rem 0.45rem;
  }

  /* The heading over a group, inset to line up with the rows' labels — the
     settings menu's, which groups its themes the same way. */
  .menu-title {
    padding: 0.5rem var(--menu-item-pad-x) 0.3rem;
    font-size: var(--text-3xs);
    font-weight: var(--weight-semibold);
    color: var(--fg-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
</style>
