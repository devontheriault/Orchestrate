<script lang="ts">
  import { tick } from "svelte";
  import { theme, THEMES, type ThemePref } from "$lib/theme/theme.svelte";
  import { usage } from "$lib/usage/usage.svelte";
  import { api } from "$lib/api";
  import { store } from "$lib/state/store.svelte";
  import { hosts } from "$lib/state/hosts.svelte";
  import HostsDialog from "./HostsDialog.svelte";
  import {
    dismissOnMove,
    menuStyle,
    opensMenu,
    placeMenu,
    placeSubmenu,
    stepActive,
    type Placement,
  } from "$lib/menus/menu";

  /** Rail mode: icon only, for windows too narrow to spare the width. */
  let { collapsed = false }: { collapsed?: boolean } = $props();

  const uid = $props.id();

  /**
   * The menu's own rows: Theme, which opens the themes beside it, Usage, the
   * Hosts this window talks to, and — when this machine's Host runs as a
   * service — whether it keeps the agents running while the user is logged out.
   */
  const THEME = 0;
  const USAGE = 1;
  const HOSTS = 2;
  const KEEP = 3;

  /** Asked of the Host each time the menu opens; null hides the row. */
  let keepRunning = $state<boolean | null>(null);
  const count = $derived(keepRunning === null ? 3 : 4);

  /** The Hosts dialog is up. */
  let managingHosts = $state(false);

  async function loadKeepRunning() {
    try {
      keepRunning = await api.keepRunning();
    } catch {
      keepRunning = null;
    }
  }

  async function toggleKeepRunning() {
    if (keepRunning === null) return;
    try {
      await api.setKeepRunning(!keepRunning);
    } catch (e) {
      store.error = String(e);
    }
    await loadKeepRunning();
  }

  /**
   * The submenu reads as three groups — System, the light themes, the dark
   * ones — but the keyboard walks one flat list, so `themes` is that list and
   * the groups are only how it is drawn. A theme row is worn by the whole
   * window as soon as it is pointed at, and picking one leaves the menus up:
   * the point of a theme picker is seeing the app change under it, and with
   * this many to try, closing after each would be the whole cost of trying
   * them.
   */
  const light = THEMES.filter((t) => t.mode === "light");
  const dark = THEMES.filter((t) => t.mode === "dark");
  const themes: { id: ThemePref; name: string }[] = [
    { id: "system", name: "System" },
    ...light,
    ...dark,
  ];

  /**
   * The themes submenu's width, in rem: the UI's type grows with the window,
   * so a width in pixels that fits at the smallest size cuts the names off at
   * the largest.
   */
  const SUB_WIDTH_REM = 17;

  let open = $state(false);
  /** Index the keyboard is on while the menu is up. */
  let active = $state(0);
  /** Whether the themes submenu is showing, and where the keys are in it. */
  let subOpen = $state(false);
  let subActive = $state(0);
  /** Whether the keys are driving the submenu rather than the menu. */
  let inSub = $state(false);

  let triggerEl: HTMLButtonElement | undefined = $state();
  let menuEl: HTMLElement | undefined = $state();
  let subEl: HTMLElement | undefined = $state();

  let placement = $state<Placement | null>(null);
  let subPlacement = $state<Placement | null>(null);

  function place() {
    if (!triggerEl) return;
    // This button lives at the bottom of the window, so opening upward is the
    // rule rather than the exception. Left edges line up: the footer's rows
    // are left-aligned, and in the rail the menu has nowhere to go but out to
    // the right.
    placement = placeMenu(triggerEl, { minWidth: 200, align: "left" });
  }

  function openMenu() {
    loadKeepRunning();
    active = THEME;
    closeSub();
    place();
    open = true;
  }

  function close(refocus = true) {
    if (!open) return;
    open = false;
    closeSub();
    if (refocus) triggerEl?.focus();
  }

  function openSub() {
    const row = menuEl?.querySelector<HTMLElement>(`#${CSS.escape(`${uid}-item-${THEME}`)}`);
    if (!row || !menuEl) return;
    if (!subOpen) {
      subActive = Math.max(
        0,
        themes.findIndex((t) => t.id === theme.pref),
      );
    }
    const rem = parseFloat(getComputedStyle(document.documentElement).fontSize);
    subPlacement = placeSubmenu(row, menuEl, SUB_WIDTH_REM * rem);
    subOpen = true;
  }

  function closeSub() {
    subOpen = false;
    inSub = false;
  }

  function pick(i: number) {
    if (i === USAGE) {
      close();
      usage.show();
      return;
    }
    if (i === HOSTS) {
      close();
      managingHosts = true;
      return;
    }
    // Stays open, so the tick can be seen to move.
    if (i === KEEP) {
      toggleKeepRunning();
      return;
    }
    openSub();
    inSub = true;
  }

  /**
   * Wear the theme the user is on. `subActive` is what the pointer is over and
   * what the arrow keys are on, so hover and keyboard preview the same way.
   * Closing the submenu — or the menu — drops the preview, which is what puts
   * the chosen theme back after a look around.
   */
  $effect(() => {
    if (!open || !subOpen) return;
    theme.preview = themes[subActive].id;
    return () => (theme.preview = null);
  });

  // `/theme` with no name asks for the themes: open on them, keys in the list.
  // Only a request made since this menu mounted counts, so moving between the
  // rail and the full sidebar doesn't open it again.
  let requestsSeen = theme.pickerRequests;
  $effect(() => {
    const requests = theme.pickerRequests;
    if (requests === requestsSeen) return;
    requestsSeen = requests;
    openMenu();
    tick().then(() => pick(THEME));
  });

  // The menus are pinned to the trigger's position, so anything that moves it
  // dismisses them rather than leaving them stranded mid-air.
  $effect(() => {
    if (!open) return;
    return dismissOnMove(() => [menuEl, subEl, triggerEl], () => close(false));
  });

  // Keys land on the menu itself, including while it is driving the submenu.
  $effect(() => {
    if (open) menuEl?.focus();
  });

  // Arrowing past the bottom of the scrolling themes has to bring the list
  // with it, or the lit row is one the user cannot see.
  $effect(() => {
    if (!subOpen) return;
    subEl
      ?.querySelector(`#${CSS.escape(`${uid}-theme-${subActive}`)}`)
      ?.scrollIntoView({ block: "nearest" });
  });

  function onTriggerKeydown(e: KeyboardEvent) {
    if (opensMenu(e.key)) {
      e.preventDefault();
      openMenu();
    }
  }

  function onMenuKeydown(e: KeyboardEvent) {
    // The arrows, Home and End walk whichever list the keys are in; leaving
    // the Theme row closes the submenu hanging off it.
    const next = inSub
      ? stepActive(e.key, subActive, themes.length)
      : stepActive(e.key, active, count);
    if (next !== null) {
      e.preventDefault();
      if (inSub) subActive = next;
      else {
        active = next;
        closeSub();
      }
      return;
    }
    switch (e.key) {
      case "Escape":
        e.preventDefault();
        if (inSub) closeSub();
        else close();
        break;
      case "Tab":
        close(false);
        break;
      case "ArrowRight":
        e.preventDefault();
        if (!inSub && active === THEME) pick(THEME);
        break;
      case "ArrowLeft":
        e.preventDefault();
        closeSub();
        break;
      case "Enter":
      case " ":
        e.preventDefault();
        // Stays open: the window is repainting behind the menu, which is the
        // fastest way to try a few and keep the one you like.
        if (inSub) theme.set(themes[subActive].id);
        else pick(active);
        break;
    }
  }
</script>

<button
  bind:this={triggerEl}
  type="button"
  class="btn btn-ghost settings"
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

<!-- One theme row. The menu owns the keyboard: the rows aren't focusable, so
     a key handler on each one would never fire. -->
{#snippet row(t: { id: ThemePref; name: string }, i: number)}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    id={`${uid}-theme-${i}`}
    role="menuitemradio"
    aria-checked={t.id === theme.pref}
    tabindex="-1"
    data-active={inSub && i === subActive}
    class="menu-item"
    class:on={t.id === theme.pref}
    onclick={() => theme.set(t.id)}
    onpointermove={() => {
      subActive = i;
      inSub = true;
    }}
  >
    <span class="menu-tick" aria-hidden="true">{t.id === theme.pref ? "✓" : ""}</span>
    <!-- The swatch wears the theme rather than describing it: the ring is the
         menu's, the disc inside is painted with that theme's own tokens, so a
         palette can never be previewed as something it is not. -->
    <span class="swatch" aria-hidden="true">
      <span class="swatch-fill" data-theme={t.id === "system" ? theme.system : t.id}></span>
    </span>
    <span class="menu-label">{t.name}</span>
    {#if t.id === "system"}<span class="menu-note">{theme.systemName}</span>{/if}
  </div>
{/snippet}

<!--
  Fixed position, so the panes' `overflow: hidden` can't clip it.
-->
{#if open && placement}
  <div
    bind:this={menuEl}
    class="popover menu fit"
    role="menu"
    aria-label="Settings"
    aria-activedescendant={inSub ? `${uid}-theme-${subActive}` : `${uid}-item-${active}`}
    tabindex="-1"
    onkeydown={onMenuKeydown}
    style={menuStyle(placement)}
  >
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div
      id={`${uid}-item-${THEME}`}
      role="menuitem"
      aria-haspopup="menu"
      aria-expanded={subOpen}
      tabindex="-1"
      data-active={!inSub && active === THEME}
      class="menu-item"
      class:sub-open={subOpen}
      onclick={() => pick(THEME)}
      onpointermove={() => {
        active = THEME;
        inSub = false;
        if (!subOpen) openSub();
      }}
    >
      <!-- The theme being worn, in the tick column the way Usage's gauge is. -->
      <span class="menu-tick lead" aria-hidden="true">
        <span class="swatch">
          <span class="swatch-fill" data-theme={theme.resolved}></span>
        </span>
      </span>
      <span class="menu-label">Theme</span>
      <span class="menu-note">{theme.label}</span>
      <span class="menu-more" aria-hidden="true">›</span>
    </div>

    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div
      id={`${uid}-item-${USAGE}`}
      role="menuitem"
      tabindex="-1"
      data-active={!inSub && active === USAGE}
      class="menu-item usage"
      onclick={() => pick(USAGE)}
      onpointermove={() => {
        active = USAGE;
        closeSub();
      }}
      title="Token usage (Ctrl+Shift+U)"
    >
      <span class="menu-tick lead gauge" aria-hidden="true">
        <svg viewBox="0 0 10 8" width="10" height="8">
          <rect x="0.5" y="5" width="2" height="3" rx="0.5" fill="currentColor" />
          <rect x="4" y="2.5" width="2" height="5.5" rx="0.5" fill="currentColor" />
          <rect x="7.5" y="0.5" width="2" height="7.5" rx="0.5" fill="currentColor" />
        </svg>
      </span>
      <span class="menu-label">Usage</span>
      <kbd>Ctrl + Shift + U</kbd>
    </div>

    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div
      id={`${uid}-item-${HOSTS}`}
      role="menuitem"
      tabindex="-1"
      data-active={!inSub && active === HOSTS}
      class="menu-item"
      onclick={() => pick(HOSTS)}
      onpointermove={() => {
        active = HOSTS;
        closeSub();
      }}
      title="Your other machines, whose agents this window shows too"
    >
      <span class="menu-tick" aria-hidden="true"></span>
      <span class="menu-label">Hosts…</span>
      <span class="menu-note">{hosts.list.length}</span>
    </div>

    {#if keepRunning !== null}
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <div
        id={`${uid}-item-${KEEP}`}
        role="menuitemcheckbox"
        aria-checked={keepRunning}
        tabindex="-1"
        data-active={!inSub && active === KEEP}
        class="menu-item"
        onclick={() => pick(KEEP)}
        onpointermove={() => {
          active = KEEP;
          closeSub();
        }}
        title="Agents on this machine keep working after you log out, and the Host starts at boot"
      >
        <span class="menu-tick" aria-hidden="true">{keepRunning ? "✓" : ""}</span>
        <span class="menu-label">Keep agents running when logged out</span>
      </div>
    {/if}
  </div>
{/if}

{#if managingHosts}
  <HostsDialog onclose={() => (managingHosts = false)} />
{/if}

{#if open && subOpen && subPlacement}
  <div
    bind:this={subEl}
    class="popover popover-above menu"
    role="menu"
    aria-label="Theme"
    tabindex="-1"
    style={menuStyle(subPlacement)}
  >
    <div role="group" aria-label="System">
      {@render row(themes[0], 0)}
    </div>

    <div class="menu-title" id={`${uid}-light`}>Light</div>
    <div role="group" aria-labelledby={`${uid}-light`}>
      {#each light as t, n (t.id)}{@render row(t, 1 + n)}{/each}
    </div>

    <div class="menu-title" id={`${uid}-dark`}>Dark</div>
    <div role="group" aria-labelledby={`${uid}-dark`}>
      {#each dark as t, n (t.id)}{@render row(t, 1 + light.length + n)}{/each}
    </div>
  </div>
{/if}

<style>
  /* The footer's one row: quiet, full width, and left-aligned in the rail too. Sized as
     a footer tile rather than a `.btn` in a row of buttons. */
  .settings {
    width: 100%;
    justify-content: flex-start;
    gap: var(--space-3);
    padding: 0.35rem 0.45rem;
    font-size: var(--text-sm);
    font-weight: var(--weight-normal);
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

  /* The rows are fixed wording, not a list of names to ellipsize: the menu
     grows to fit them rather than cutting them off as the type scales up. */
  .fit {
    min-width: max-content;
  }

  /* The Theme row stays lit while the pointer is off in its submenu. */
  .sub-open {
    background: var(--hover);
  }

  /* An icon standing in for the tick column, so the labels still line up. */
  .lead {
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .lead .swatch {
    width: 0.8rem;
    height: 0.8rem;
  }

  /* The heading over a group of rows: the same inset as a row's label, so the
     words line up down the menu's left edge. */
  .menu-title {
    padding: 0.5rem var(--menu-item-pad-x) 0.3rem;
    font-size: var(--text-3xs);
    font-weight: var(--weight-semibold);
    color: var(--fg-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  /* A disc of the theme: its page, its accent, its green. Three thirds rather
     than a single square of background, because two themes can share a
     background and still be nothing alike. */
  .swatch {
    flex: none;
    display: flex;
    width: 0.92rem;
    height: 0.92rem;
    border-radius: var(--radius-circle);
    /* Drawn in the menu's border colour, not the theme's, so a dark palette
       previewed in a dark menu still has an edge. */
    border: var(--border-width) solid var(--border);
  }

  .swatch-fill {
    width: 100%;
    height: 100%;
    border-radius: inherit;
    background: conic-gradient(
      from 0.5turn,
      var(--surface) 0 33.34%,
      var(--accent) 33.34% 66.67%,
      var(--success) 66.67% 100%
    );
  }

  .gauge {
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
