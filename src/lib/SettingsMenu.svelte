<script lang="ts">
  import { theme, type ThemePref } from "./theme.svelte";
  import { usage } from "./usage.svelte";

  /** Rail mode: icon only, for windows too narrow to spare the width. */
  let { collapsed = false }: { collapsed?: boolean } = $props();

  const uid = $props.id();

  /**
   * One flat list, so the keyboard walks it without a mode: the three themes,
   * then the actions. A theme row is worn by the whole window as soon as it is
   * pointed at, and picking one is the only row that leaves the menu up — the
   * point of a theme picker is seeing the app change under it.
   */
  const themes: { id: ThemePref; name: string }[] = [
    { id: "system", name: "System" },
    { id: "light", name: "Light" },
    { id: "dark", name: "Dark" },
  ];

  const COUNT = themes.length + 1;
  /** The Usage row sits after the themes. */
  const USAGE = themes.length;

  let open = $state(false);
  /** Index the keyboard is on while the menu is up. */
  let active = $state(0);

  let triggerEl: HTMLButtonElement | undefined = $state();
  let menuEl: HTMLElement | undefined = $state();

  /** Where the fixed menu sits — measured off the trigger, flipped if needed. */
  type Placement = { left: number; width: number; y: string };
  let placement = $state<Placement | null>(null);

  const GAP = 6;
  const EDGE = 8;
  const MENU_MIN_WIDTH = 194;
  /** Four rows and the separator; enough to decide which way to open. */
  const MENU_HEIGHT = 178;

  function place() {
    if (!triggerEl) return;
    const r = triggerEl.getBoundingClientRect();
    const below = window.innerHeight - r.bottom - GAP - EDGE;
    // This button lives at the bottom of the window, so opening upward is the
    // rule rather than the exception.
    const up = below < MENU_HEIGHT;
    const width = Math.max(r.width, MENU_MIN_WIDTH);
    placement = {
      width,
      // Left edges line up: the footer's rows are left-aligned, and in the rail
      // the menu has nowhere to go but out to the right.
      left: Math.min(Math.max(EDGE, r.left), window.innerWidth - width - EDGE),
      y: up
        ? `bottom: ${Math.round(window.innerHeight - r.top + GAP)}px`
        : `top: ${Math.round(r.bottom + GAP)}px`,
    };
  }

  function openMenu() {
    active = Math.max(
      0,
      themes.findIndex((t) => t.id === theme.pref),
    );
    place();
    open = true;
  }

  function close(refocus = true) {
    if (!open) return;
    open = false;
    if (refocus) triggerEl?.focus();
  }

  function pick(i: number) {
    if (i === USAGE) {
      close();
      usage.show();
      return;
    }
    // Stays open: the window is repainting behind the menu, which is the
    // fastest way to try the three and keep the one you like.
    theme.set(themes[i].id);
  }

  /**
   * Wear the row the user is on. `active` is what the pointer is over and what
   * the arrow keys are on, so hover and keyboard preview the same way, and the
   * rows that aren't themes (Usage) drop the preview rather than freezing it.
   * Clearing on close is what puts the chosen theme back after a look around.
   */
  $effect(() => {
    if (!open) return;
    theme.preview = active < themes.length ? themes[active].id : null;
    return () => (theme.preview = null);
  });

  // The menu is pinned to the trigger's position, so anything that moves it
  // dismisses the menu rather than leaving it stranded mid-air.
  $effect(() => {
    if (!open) return;
    const ours = (t: Node | null) =>
      !!t && (!!menuEl?.contains(t) || !!triggerEl?.contains(t));
    const onDown = (e: PointerEvent) => {
      if (ours(e.target as Node)) return;
      close(false);
    };
    const onScroll = (e: Event) => {
      if (ours(e.target as Node)) return;
      close(false);
    };
    const onResize = () => close(false);
    window.addEventListener("pointerdown", onDown, true);
    window.addEventListener("resize", onResize);
    // Capture: the project list scrolls, not the window.
    window.addEventListener("scroll", onScroll, true);
    return () => {
      window.removeEventListener("pointerdown", onDown, true);
      window.removeEventListener("resize", onResize);
      window.removeEventListener("scroll", onScroll, true);
    };
  });

  // Keys land on the menu itself, per the codebase's other pickers.
  $effect(() => {
    if (open) menuEl?.focus();
  });

  function onTriggerKeydown(e: KeyboardEvent) {
    if (e.key === "ArrowUp" || e.key === "ArrowDown" || e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      openMenu();
    }
  }

  function onMenuKeydown(e: KeyboardEvent) {
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
        active = (active + 1) % COUNT;
        break;
      case "ArrowUp":
        e.preventDefault();
        active = (active - 1 + COUNT) % COUNT;
        break;
      case "Home":
        e.preventDefault();
        active = 0;
        break;
      case "End":
        e.preventDefault();
        active = COUNT - 1;
        break;
      case "Enter":
      case " ":
        e.preventDefault();
        pick(active);
        break;
    }
  }
</script>

<button
  bind:this={triggerEl}
  type="button"
  class="btn btn-ghost settings"
  class:collapsed
  class:on={open}
  aria-haspopup="menu"
  aria-expanded={open}
  aria-label="Settings"
  title={collapsed ? `Settings — theme: ${theme.label}` : "Settings"}
  onclick={() => (open ? close() : openMenu())}
  onkeydown={onTriggerKeydown}
>
  <span class="icon" aria-hidden="true">
    <!-- Sliders rather than a gear: at 13px a gear's teeth turn to mud. -->
    <svg viewBox="0 0 16 16" width="13" height="13">
      <path
        d="M1.5 5h13M1.5 11h13"
        fill="none"
        stroke="currentColor"
        stroke-width="1.4"
        stroke-linecap="round"
      />
      <circle cx="5.5" cy="5" r="2" fill="var(--panel-bg)" stroke="currentColor" stroke-width="1.4" />
      <circle cx="10.5" cy="11" r="2" fill="var(--panel-bg)" stroke="currentColor" stroke-width="1.4" />
    </svg>
  </span>
  {#if !collapsed}<span class="label">Settings</span>{/if}
</button>

<!--
  Fixed position, so the panes' `overflow: hidden` can't clip it.
-->
{#if open && placement}
  <div
    bind:this={menuEl}
    class="popover menu settings-menu"
    role="menu"
    aria-label="Settings"
    aria-activedescendant={`${uid}-item-${active}`}
    tabindex="-1"
    onkeydown={onMenuKeydown}
    style={`left: ${Math.round(placement.left)}px; width: ${Math.round(placement.width)}px; ${placement.y}`}
  >
    <div class="menu-title" id={`${uid}-theme`}>Theme</div>
    <div role="group" aria-labelledby={`${uid}-theme`}>
      {#each themes as t, i (t.id)}
        <!-- The menu owns the keyboard: the rows aren't focusable, so a key
             handler on each one would never fire. -->
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div
          id={`${uid}-item-${i}`}
          role="menuitemradio"
          aria-checked={t.id === theme.pref}
          tabindex="-1"
          data-active={i === active}
          class="menu-item"
          class:on={t.id === theme.pref}
          onclick={() => pick(i)}
          onpointermove={() => (active = i)}
        >
          <span class="menu-tick" aria-hidden="true">{t.id === theme.pref ? "✓" : ""}</span>
          <span class="menu-label">{t.name}</span>
          {#if t.id === "system"}<span class="menu-note">{theme.system}</span>{/if}
        </div>
      {/each}
    </div>

    <div class="menu-sep" role="none"></div>

    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div
      id={`${uid}-item-${USAGE}`}
      role="menuitem"
      tabindex="-1"
      data-active={USAGE === active}
      class="menu-item usage"
      onclick={() => pick(USAGE)}
      onpointermove={() => (active = USAGE)}
      title="Token usage (Ctrl+Shift+U)"
    >
      <span class="menu-tick gauge" aria-hidden="true">
        <svg viewBox="0 0 10 8" width="10" height="8">
          <rect x="0.5" y="5" width="2" height="3" rx="0.5" fill="currentColor" />
          <rect x="4" y="2.5" width="2" height="5.5" rx="0.5" fill="currentColor" />
          <rect x="7.5" y="0.5" width="2" height="7.5" rx="0.5" fill="currentColor" />
        </svg>
      </span>
      <span class="menu-label">Usage</span>
      <kbd>Ctrl + Shift + U</kbd>
    </div>
  </div>
{/if}

<style>
  /* The footer's one row: quiet, full width, and square in the rail. Sized as
     a footer tile rather than a `.btn` in a row of buttons. */
  .settings {
    width: 100%;
    justify-content: flex-start;
    gap: var(--space-3);
    padding: 0.35rem 0.45rem;
    font-size: var(--text-sm);
    font-weight: var(--weight-normal);
  }

  .settings.collapsed {
    justify-content: center;
    padding: 0.35rem 0;
  }

  /* Held lit while the menu it opened is up. */
  .settings.on {
    background: var(--hover);
    color: var(--fg);
  }

  .icon {
    display: flex;
    align-items: center;
    line-height: var(--leading-none);
  }

  .label {
    flex: 1;
    text-align: left;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* The heading over a group of rows: the same inset as a row's label, so the
     words line up down the menu's left edge. */
  .menu-title {
    padding: 0.2rem var(--menu-item-pad-x) 0.3rem;
    font-size: var(--text-3xs);
    font-weight: var(--weight-semibold);
    color: var(--fg-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  /* The gauge stands in for the tick column, so the labels still line up. */
  .gauge {
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--fg-muted);
  }

  .usage[data-active="true"] .gauge {
    color: var(--fg);
  }

  /* The shortcut is a reminder, not a label: it keeps its space in the row so
     nothing shifts, and only surfaces when the row is pointed at. */
  .usage kbd {
    flex: none;
    font-family: var(--font-mono);
    font-size: var(--text-3xs);
    white-space: nowrap;
    color: var(--fg-muted);
    background: var(--code-bg);
    border-radius: var(--radius-xs);
    padding: 0.05em 0.3em;
    opacity: 0;
    transition: opacity var(--transition-fast);
  }

  .usage:hover kbd,
  .usage[data-active="true"] kbd {
    opacity: 1;
  }
</style>
