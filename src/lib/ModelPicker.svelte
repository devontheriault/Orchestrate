<script lang="ts">
  import { DEFAULT_EFFORT, DEFAULT_MODEL, EFFORTS, modelLabel } from "./api";
  import { store } from "./store.svelte";

  let {
    value = $bindable(DEFAULT_MODEL),
    effort = $bindable(DEFAULT_EFFORT),
    disabled = false,
    /** Sized for the composer strip rather than a dialog. */
    compact = false,
    label = "Model",
  }: {
    value?: string;
    effort?: string;
    disabled?: boolean;
    compact?: boolean;
    label?: string;
  } = $props();

  const uid = $props.id();

  // A value the account no longer offers still gets an entry, so an agent that
  // ran on a retired model shows what it ran on instead of silently reading as
  // something else.
  const options = $derived.by(() => {
    const listed = store.models.map((m) => ({
      id: m.id,
      name: modelLabel(m.display_name),
    }));
    const known = value === DEFAULT_MODEL || listed.some((o) => o.id === value);
    return [
      { id: DEFAULT_MODEL, name: "Default" },
      ...listed,
      ...(known ? [] : [{ id: value, name: value }]),
    ];
  });

  const selected = $derived(options.find((o) => o.id === value) ?? options[0]);

  /** `claude --effort` takes these verbatim; the menu just title-cases them. */
  const efforts = [
    { id: DEFAULT_EFFORT, name: "Default" },
    ...EFFORTS.map((id) => ({
      id,
      name: id === "xhigh" ? "XHigh" : id[0].toUpperCase() + id.slice(1),
    })),
  ];

  let open = $state(false);
  /** Index the keyboard is on while the menu is up. */
  let active = $state(0);
  /** Which model row's effort submenu is showing, and where the keys are in it. */
  let sub = $state<number | null>(null);
  let subActive = $state(0);
  /** Whether the keys are driving the submenu rather than the model list. */
  let inSub = $state(false);
  /**
   * Whether the last move came from the keyboard. Only then is it right to
   * scroll the list — doing it on hover fires a scroll the dismisser sees.
   */
  let keyNav = $state(false);

  let triggerEl: HTMLButtonElement | undefined = $state();
  let listEl: HTMLElement | undefined = $state();
  let subEl: HTMLElement | undefined = $state();

  /** Where a fixed menu sits — anchored to what opened it, flipped if needed. */
  type Placement = { left: number; width: number; maxHeight: number; y: string };
  let placement = $state<Placement | null>(null);
  let subPlacement = $state<Placement | null>(null);

  const GAP = 6;
  const EDGE = 8;
  const MENU_MIN_WIDTH = 176;
  const MENU_MAX_HEIGHT = 280;
  const SUB_WIDTH = 132;

  function place() {
    if (!triggerEl) return;
    const r = triggerEl.getBoundingClientRect();
    const below = window.innerHeight - r.bottom - GAP - EDGE;
    const above = r.top - GAP - EDGE;
    // The composer's picker lives at the bottom of the window, so dropping
    // upward is the common case rather than the exception.
    const up = below < Math.min(MENU_MAX_HEIGHT, above);
    const width = Math.max(r.width, MENU_MIN_WIDTH);
    placement = {
      width,
      // Right edges line up: that's the edge the trigger is aligned on.
      left: Math.min(Math.max(EDGE, r.right - width), window.innerWidth - width - EDGE),
      maxHeight: Math.max(120, Math.min(MENU_MAX_HEIGHT, up ? above : below)),
      y: up
        ? `bottom: ${Math.round(window.innerHeight - r.top + GAP)}px`
        : `top: ${Math.round(r.bottom + GAP)}px`,
    };
  }

  /** Hang the effort list off a model row, on whichever side has the room. */
  function placeSub(row: HTMLElement) {
    const r = row.getBoundingClientRect();
    const menu = listEl?.getBoundingClientRect() ?? r;
    // No gap: the pointer has to cross from the row into the submenu, and a
    // gap there is a dead zone that closes it mid-travel.
    const right = menu.right;
    const left =
      right + SUB_WIDTH <= window.innerWidth - EDGE ? right : menu.left - SUB_WIDTH;
    const height = efforts.length * 30 + 10;
    subPlacement = {
      left: Math.max(EDGE, left),
      width: SUB_WIDTH,
      maxHeight: height,
      y: `top: ${Math.round(Math.min(Math.max(EDGE, r.top - 5), window.innerHeight - height - EDGE))}px`,
    };
  }

  function openSub(i: number, row: HTMLElement) {
    sub = i;
    subActive = Math.max(
      0,
      efforts.findIndex((e) => e.id === effort),
    );
    placeSub(row);
  }

  function closeSub() {
    sub = null;
    inSub = false;
  }

  function openMenu() {
    if (disabled) return;
    active = Math.max(
      0,
      options.findIndex((o) => o.id === value),
    );
    closeSub();
    keyNav = true;
    place();
    open = true;
  }

  function close(refocus = true) {
    if (!open) return;
    open = false;
    closeSub();
    if (refocus) triggerEl?.focus();
  }

  /** Commit a row — and, from the submenu, the effort chosen on it. */
  function pick(model: string, level: string = effort) {
    value = model;
    effort = level;
    close();
  }

  // The menus are pinned to the trigger's position, so anything that moves it
  // dismisses them rather than leaving them stranded mid-air.
  $effect(() => {
    if (!open) return;
    const ours = (t: Node | null) =>
      !!t && (!!listEl?.contains(t) || !!subEl?.contains(t) || !!triggerEl?.contains(t));
    const onDown = (e: PointerEvent) => {
      if (ours(e.target as Node)) return;
      close(false);
    };
    const onScroll = (e: Event) => {
      // A menu scrolling inside itself isn't the page moving under it.
      if (ours(e.target as Node)) return;
      close(false);
    };
    const onResize = () => close(false);
    window.addEventListener("pointerdown", onDown, true);
    window.addEventListener("resize", onResize);
    // Capture: the transcript and project list scroll, not the window.
    window.addEventListener("scroll", onScroll, true);
    return () => {
      window.removeEventListener("pointerdown", onDown, true);
      window.removeEventListener("resize", onResize);
      window.removeEventListener("scroll", onScroll, true);
    };
  });

  // Keys land on the model list, including while it is driving the submenu.
  $effect(() => {
    if (open) listEl?.focus();
  });

  // A menu taller than its box opens on the current choice, not at the top.
  $effect(() => {
    if (!open || !keyNav) return;
    active;
    listEl
      ?.querySelector<HTMLElement>('[data-active="true"]')
      ?.scrollIntoView({ block: "nearest" });
  });

  function onTriggerKeydown(e: KeyboardEvent) {
    if (e.key === "ArrowDown" || e.key === "ArrowUp" || e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      openMenu();
    }
  }

  function rowEl(i: number): HTMLElement | null {
    return listEl?.querySelector<HTMLElement>(`#${CSS.escape(`${uid}-opt-${i}`)}`) ?? null;
  }

  function onListKeydown(e: KeyboardEvent) {
    keyNav = true;
    switch (e.key) {
      case "Escape":
        e.preventDefault();
        if (inSub) closeSub();
        else close();
        break;
      case "Tab":
        close(false);
        break;
      case "ArrowDown":
        e.preventDefault();
        if (inSub) subActive = (subActive + 1) % efforts.length;
        else {
          active = (active + 1) % options.length;
          closeSub();
        }
        break;
      case "ArrowUp":
        e.preventDefault();
        if (inSub) subActive = (subActive - 1 + efforts.length) % efforts.length;
        else {
          active = (active - 1 + options.length) % options.length;
          closeSub();
        }
        break;
      case "ArrowRight": {
        e.preventDefault();
        const row = rowEl(active);
        if (row) {
          openSub(active, row);
          inSub = true;
        }
        break;
      }
      case "ArrowLeft":
        e.preventDefault();
        closeSub();
        break;
      case "Home":
        e.preventDefault();
        if (inSub) subActive = 0;
        else {
          active = 0;
          closeSub();
        }
        break;
      case "End":
        e.preventDefault();
        if (inSub) subActive = efforts.length - 1;
        else {
          active = options.length - 1;
          closeSub();
        }
        break;
      case "Enter":
      case " ":
        e.preventDefault();
        if (inSub && sub !== null) pick(options[sub].id, efforts[subActive].id);
        else pick(options[active].id);
        break;
    }
  }
</script>

<span class="picker" class:compact>
  <button
    bind:this={triggerEl}
    type="button"
    class="btn btn-select trigger"
    class:btn-ghost={compact}
    {disabled}
    aria-haspopup="listbox"
    aria-expanded={open}
    aria-label={label}
    title={effort ? `${label} — ${effort} effort` : label}
    onclick={() => (open ? close() : openMenu())}
    onkeydown={onTriggerKeydown}
  >
    <span class="btn-select-label">{selected?.name ?? value}</span>
    <!-- The effort rides along with the model rather than claiming a control of
         its own: quieter than the name it qualifies. -->
    {#if effort}<span class="effort">{effort}</span>{/if}
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

<!--
  A native <select> hands its open list to the OS, which draws square corners in
  the desktop theme's colours, ignores everything this stylesheet says, and has
  no submenu to hang the effort off. The lists below are ours. Fixed position
  keeps the panes' `overflow: hidden` from clipping them.
-->
{#if open && placement}
  <ul
    bind:this={listEl}
    class="popover menu"
    role="listbox"
    aria-label={label}
    aria-activedescendant={inSub
      ? `${uid}-eff-${subActive}`
      : `${uid}-opt-${active}`}
    tabindex="-1"
    onkeydown={onListKeydown}
    style={`left: ${Math.round(placement.left)}px; width: ${Math.round(placement.width)}px; max-height: ${Math.round(placement.maxHeight)}px; ${placement.y}`}
  >
    {#each options as option, i (option.id)}
      <!-- The listbox owns the keyboard, per the ARIA pattern: the rows aren't
           focusable, so a key handler on each one would never fire. -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <li
        id={`${uid}-opt-${i}`}
        role="option"
        aria-selected={option.id === value}
        data-active={!inSub && i === active}
        class="menu-item"
        class:on={option.id === value}
        class:sub-open={sub === i}
        onclick={() => pick(option.id)}
        onpointermove={(e) => {
          keyNav = false;
          active = i;
          if (sub !== i) openSub(i, e.currentTarget as HTMLElement);
          inSub = false;
        }}
      >
        <span class="menu-tick" aria-hidden="true">{option.id === value ? "✓" : ""}</span>
        <span class="menu-label">{option.name}</span>
        <span class="menu-more" aria-hidden="true">›</span>
      </li>
    {/each}
  </ul>
{/if}

{#if open && sub !== null && subPlacement}
  <ul
    bind:this={subEl}
    class="popover popover-above menu"
    role="listbox"
    aria-label={`Effort for ${options[sub].name}`}
    tabindex="-1"
    style={`left: ${Math.round(subPlacement.left)}px; width: ${Math.round(subPlacement.width)}px; ${subPlacement.y}`}
  >
    {#each efforts as level, j (level.id)}
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <li
        id={`${uid}-eff-${j}`}
        role="option"
        aria-selected={level.id === effort}
        data-active={inSub && j === subActive}
        class="menu-item"
        class:on={level.id === effort}
        onclick={() => pick(options[sub!].id, level.id)}
        onpointermove={() => {
          keyNav = false;
          subActive = j;
          inSub = true;
        }}
      >
        <span class="menu-tick" aria-hidden="true">{level.id === effort ? "✓" : ""}</span>
        <span class="menu-label">{level.name}</span>
      </li>
    {/each}
  </ul>
{/if}

<style>
  .picker {
    position: relative;
    display: inline-flex;
    align-items: center;
    min-width: 0;
  }

  /* Wider and taller than the app's select: this one is the spawn form's
     main control, so it fills its row and takes the form's type size. */
  .trigger {
    width: 100%;
    align-items: baseline;
    font-size: var(--text-lg);
    padding: 0.5rem 0.7rem;
  }

  .effort {
    flex: none;
    opacity: 0.55;
    font-size: 0.92em;
  }

  /* The row whose submenu is up stays lit while the pointer is off in it. */
  .sub-open {
    background: var(--hover);
  }

  /* Under the composer box this is a quiet secondary control rather than an
     input to fill in — `.btn-ghost` does the rest. A long model name ellipses
     rather than pushing it off a narrow pane. */
  .compact {
    max-width: 13rem;
  }

  .compact .trigger {
    font-size: var(--text-xs);
    padding: 0.25rem 0.45rem;
  }
</style>
