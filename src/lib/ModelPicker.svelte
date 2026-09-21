<script lang="ts">
  import { DEFAULT_MODEL } from "./api";
  import { store } from "./store.svelte";

  let {
    value = $bindable(DEFAULT_MODEL),
    disabled = false,
    /** Sized for the composer strip rather than a dialog. */
    compact = false,
    label = "Model",
  }: {
    value?: string;
    disabled?: boolean;
    compact?: boolean;
    label?: string;
  } = $props();

  const uid = $props.id();

  // A value the account no longer offers still gets an entry, so an agent that
  // ran on a retired model shows what it ran on instead of silently reading as
  // something else.
  const options = $derived.by(() => {
    const listed = store.models.map((m) => ({ id: m.id, name: m.display_name }));
    const known = value === DEFAULT_MODEL || listed.some((o) => o.id === value);
    return [
      { id: DEFAULT_MODEL, name: "Default" },
      ...listed,
      ...(known ? [] : [{ id: value, name: value }]),
    ];
  });

  const selected = $derived(options.find((o) => o.id === value) ?? options[0]);

  let open = $state(false);
  /** Index the keyboard is on while the menu is up. */
  let active = $state(0);
  let triggerEl: HTMLButtonElement | undefined = $state();
  let listEl: HTMLElement | undefined = $state();

  /** Where the fixed menu sits — anchored to the trigger, flipped if needed. */
  type Placement = { left: number; width: number; maxHeight: number; y: string };
  let placement = $state<Placement | null>(null);

  const GAP = 6;
  const EDGE = 8;
  const MENU_MIN_WIDTH = 176;
  const MENU_MAX_HEIGHT = 280;

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

  function openMenu() {
    if (disabled) return;
    active = Math.max(
      0,
      options.findIndex((o) => o.id === value),
    );
    place();
    open = true;
  }

  function close(refocus = true) {
    if (!open) return;
    open = false;
    if (refocus) triggerEl?.focus();
  }

  function pick(id: string) {
    value = id;
    close();
  }

  // The menu is pinned to the trigger's position, so anything that moves it
  // dismisses the menu rather than leaving it stranded mid-air.
  $effect(() => {
    if (!open) return;
    const onDown = (e: PointerEvent) => {
      const t = e.target as Node;
      if (listEl?.contains(t) || triggerEl?.contains(t)) return;
      close(false);
    };
    const onMove = () => close(false);
    window.addEventListener("pointerdown", onDown, true);
    window.addEventListener("resize", onMove);
    // Capture: the transcript and project list scroll, not the window.
    window.addEventListener("scroll", onMove, true);
    return () => {
      window.removeEventListener("pointerdown", onDown, true);
      window.removeEventListener("resize", onMove);
      window.removeEventListener("scroll", onMove, true);
    };
  });

  // Keys land on the list, so it takes the focus while it's up.
  $effect(() => {
    if (open) listEl?.focus();
  });

  // A menu taller than its box opens on the current choice, not at the top.
  $effect(() => {
    if (!open) return;
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

  function onListKeydown(e: KeyboardEvent) {
    switch (e.key) {
      case "Escape":
        e.preventDefault();
        close();
        break;
      case "Tab":
        close(false);
        break;
      case "ArrowDown":
        e.preventDefault();
        active = (active + 1) % options.length;
        break;
      case "ArrowUp":
        e.preventDefault();
        active = (active - 1 + options.length) % options.length;
        break;
      case "Home":
        e.preventDefault();
        active = 0;
        break;
      case "End":
        e.preventDefault();
        active = options.length - 1;
        break;
      case "Enter":
      case " ":
        e.preventDefault();
        pick(options[active].id);
        break;
    }
  }
</script>

<span class="picker" class:compact>
  <button
    bind:this={triggerEl}
    type="button"
    class="trigger"
    {disabled}
    aria-haspopup="listbox"
    aria-expanded={open}
    aria-label={label}
    title={label}
    onclick={() => (open ? close() : openMenu())}
    onkeydown={onTriggerKeydown}
  >
    <span class="current">{selected?.name ?? value}</span>
    <span class="chevron" aria-hidden="true">
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
  the desktop theme's colours and ignores everything this stylesheet says. The
  list below is ours, so it can match the composer it drops out of. Fixed
  position keeps the panes' `overflow: hidden` from clipping it.
-->
{#if open && placement}
  <ul
    bind:this={listEl}
    class="menu"
    role="listbox"
    aria-label={label}
    aria-activedescendant={`${uid}-opt-${active}`}
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
        data-active={i === active}
        class:on={option.id === value}
        onclick={() => pick(option.id)}
        onpointermove={() => (active = i)}
      >
        <span class="tick" aria-hidden="true">{option.id === value ? "✓" : ""}</span>
        <span class="name">{option.name}</span>
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

  /*
   * Restates the surface, border, and radius the inputs beside it use — the
   * control this replaces was drawn by the platform and read as a foreign
   * element pasted into the page.
   */
  .trigger {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    width: 100%;
    min-width: 0;
    font-family: inherit;
    font-size: 0.88rem;
    line-height: 1.4;
    color: var(--fg);
    background: var(--panel-bg);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 0.5rem 0.7rem;
    cursor: pointer;
  }

  .trigger:hover:not(:disabled) {
    border-color: var(--accent);
  }

  .trigger:focus-visible {
    outline: none;
    border-color: var(--accent);
    box-shadow: 0 0 0 3px rgba(59, 130, 246, 0.15);
  }

  .trigger:disabled {
    opacity: 0.55;
    cursor: not-allowed;
  }

  .current {
    flex: 1;
    min-width: 0;
    text-align: left;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .chevron {
    flex: none;
    display: flex;
    /* Drawn in currentColor, so it follows the theme instead of needing a
       second copy of the icon for dark mode. */
    color: var(--fg-muted);
  }

  .menu {
    position: fixed;
    z-index: 60;
    margin: 0;
    padding: 0.25rem;
    list-style: none;
    overflow-y: auto;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 10px;
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.18);
  }

  .menu:focus {
    outline: none;
  }

  .menu li {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.38rem 0.5rem;
    /* Rounded rows inside a rounded box: a square highlight in the corners is
       the tell that this is a list bolted into a panel. */
    border-radius: 6px;
    font-size: 0.82rem;
    color: var(--fg);
    cursor: pointer;
  }

  .menu li[data-active="true"] {
    background: var(--hover);
  }

  .menu li.on {
    color: var(--accent);
  }

  .tick {
    flex: none;
    width: 0.8rem;
    font-size: 0.72rem;
  }

  .menu .name {
    flex: 1;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* Under the composer box this is a quiet secondary control, not an input to
     fill in: no border or fill until it's pointed at. A long model name
     ellipses rather than pushing it off a narrow pane. */
  .compact {
    max-width: 10rem;
  }

  .compact .trigger {
    font-size: 0.75rem;
    padding: 0.25rem 0.45rem;
    border-color: transparent;
    background: none;
    color: var(--fg-muted);
  }

  .compact .trigger:hover:not(:disabled) {
    background: var(--hover);
    border-color: transparent;
    color: var(--fg);
  }
</style>
