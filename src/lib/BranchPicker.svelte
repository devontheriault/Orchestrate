<script lang="ts">
  /**
   * The merge target, picked from the project's branches. Same control as the
   * model picker — trigger, menu, keys — with no submenu hanging off the rows:
   * a branch has nothing to qualify.
   */
  let {
    value = $bindable(""),
    options = [],
    disabled = false,
    label = "Branch",
    /** Marked in the list as the branch the project is checked out on. */
    current = null,
  }: {
    value?: string;
    options?: string[];
    disabled?: boolean;
    label?: string;
    current?: string | null;
  } = $props();

  const uid = $props.id();

  let open = $state(false);
  /** Index the keyboard is on while the menu is up. */
  let active = $state(0);
  /**
   * Whether the last move came from the keyboard. Only then is it right to
   * scroll the list — doing it on hover fires a scroll the dismisser sees.
   */
  let keyNav = $state(false);

  let triggerEl: HTMLButtonElement | undefined = $state();
  let listEl: HTMLElement | undefined = $state();

  /** Where a fixed menu sits — anchored to what opened it, flipped if needed. */
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
    // The merge box sits low in a scrolling pane, so dropping upward is as
    // likely as down.
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
    if (disabled || options.length === 0) return;
    active = Math.max(0, options.indexOf(value));
    keyNav = true;
    place();
    open = true;
  }

  function close(refocus = true) {
    if (!open) return;
    open = false;
    if (refocus) triggerEl?.focus();
  }

  function pick(branch: string) {
    value = branch;
    close();
  }

  // The menu is pinned to the trigger's position, so anything that moves it
  // dismisses it rather than leaving it stranded mid-air.
  $effect(() => {
    if (!open) return;
    const ours = (t: Node | null) =>
      !!t && (!!listEl?.contains(t) || !!triggerEl?.contains(t));
    const onDown = (e: PointerEvent) => {
      if (ours(e.target as Node)) return;
      close(false);
    };
    const onScroll = (e: Event) => {
      // The menu scrolling inside itself isn't the page moving under it.
      if (ours(e.target as Node)) return;
      close(false);
    };
    const onResize = () => close(false);
    window.addEventListener("pointerdown", onDown, true);
    window.addEventListener("resize", onResize);
    // Capture: the diff pane scrolls, not the window.
    window.addEventListener("scroll", onScroll, true);
    return () => {
      window.removeEventListener("pointerdown", onDown, true);
      window.removeEventListener("resize", onResize);
      window.removeEventListener("scroll", onScroll, true);
    };
  });

  // Keys land on the list, per the ARIA listbox pattern.
  $effect(() => {
    if (open) listEl?.focus();
  });

  // A list longer than its box opens on the current choice, not at the top.
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

  function onListKeydown(e: KeyboardEvent) {
    keyNav = true;
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
        pick(options[active]);
        break;
    }
  }
</script>

<span class="picker">
  <button
    bind:this={triggerEl}
    type="button"
    class="trigger"
    disabled={disabled || options.length === 0}
    aria-haspopup="listbox"
    aria-expanded={open}
    aria-label={label}
    title={value ? `${label} — ${value}` : label}
    onclick={() => (open ? close() : openMenu())}
    onkeydown={onTriggerKeydown}
  >
    <span class="current">{value || "No branches"}</span>
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
  Ours rather than a native <select>, for the reason the model picker gives:
  the OS draws its list in the desktop theme's colours and ignores everything
  this stylesheet says. Fixed position keeps the panes' `overflow` from
  clipping it.
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
    {#each options as name, i (name)}
      <!-- The listbox owns the keyboard: the rows aren't focusable, so a key
           handler on each one would never fire. -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <li
        id={`${uid}-opt-${i}`}
        role="option"
        aria-selected={name === value}
        data-active={i === active}
        class:on={name === value}
        onclick={() => pick(name)}
        onpointermove={() => {
          keyNav = false;
          active = i;
        }}
      >
        <span class="tick" aria-hidden="true">{name === value ? "✓" : ""}</span>
        <span class="name">{name}</span>
        <!-- Which branch the project itself is on: the one where a merge shows
             up in the user's own checkout right away. -->
        {#if name === current}<span class="note">checked out</span>{/if}
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

  /* The model picker's trigger, sized for the merge row: same surface, border,
     radius and chevron, so the two read as one control in two places. */
  .trigger {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    max-width: 14rem;
    min-width: 0;
    font-family: inherit;
    font-size: 0.83rem;
    line-height: 1.4;
    color: var(--fg);
    background: var(--panel-bg);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 0.3rem 0.5rem;
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
    opacity: 0.5;
    cursor: not-allowed;
  }

  .current {
    min-width: 0;
    text-align: left;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .chevron {
    flex: none;
    margin-left: auto;
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

  .note {
    flex: none;
    color: var(--fg-muted);
    font-size: 0.72rem;
  }
</style>
