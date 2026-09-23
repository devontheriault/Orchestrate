<script lang="ts">
  /**
   * The Permission Mode a Turn runs in — what the agent may do without asking.
   * Same control as the model picker beside it, minus the submenu: a mode has
   * nothing hanging off it. Two choices today, so the menu is short, but it is
   * a menu rather than a toggle because "YOLO / Plan" says what each one is
   * and a switch only ever says "on".
   */
  import { DEFAULT_MODE, MODES } from "$lib/picks";
  import { dismissOnMove, menuStyle, opensMenu, placeMenu, stepActive, type Placement } from "./menu";

  let {
    value = $bindable(DEFAULT_MODE),
    disabled = false,
    /** Sized for the composer strip rather than a dialog. */
    compact = false,
    label = "Mode",
  }: {
    value?: string;
    disabled?: boolean;
    compact?: boolean;
    label?: string;
  } = $props();

  const uid = $props.id();

  // A mode this build doesn't know still gets a row, so a record written by a
  // newer version reads as what it is rather than silently as YOLO.
  const options = $derived.by(() => {
    const known = MODES.some((m) => m.id === value);
    return [
      ...MODES.map((m) => ({ id: m.id, name: m.name, note: m.note })),
      ...(known ? [] : [{ id: value, name: value, note: "" }]),
    ];
  });

  const selected = $derived(options.find((o) => o.id === value) ?? options[0]);

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

  let placement = $state<Placement | null>(null);

  function place() {
    if (!triggerEl) return;
    // Like the model picker, this one lives at the bottom of the window, so
    // dropping upward is the common case rather than the exception. Wider
    // than the trigger: each row carries a line saying what the mode does.
    placement = placeMenu(triggerEl, { minWidth: 224, maxHeight: 280, align: "right" });
  }

  function openMenu() {
    if (disabled) return;
    active = Math.max(
      0,
      options.findIndex((o) => o.id === value),
    );
    keyNav = true;
    place();
    open = true;
  }

  function close(refocus = true) {
    if (!open) return;
    open = false;
    if (refocus) triggerEl?.focus();
  }

  function pick(mode: string) {
    value = mode;
    close();
  }

  // The menu is pinned to the trigger's position, so anything that moves it
  // dismisses it rather than leaving it stranded mid-air.
  $effect(() => {
    if (!open) return;
    return dismissOnMove(() => [listEl, triggerEl], () => close(false));
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
    if (opensMenu(e.key)) {
      e.preventDefault();
      openMenu();
    }
  }

  function onListKeydown(e: KeyboardEvent) {
    keyNav = true;
    const next = stepActive(e.key, active, options.length);
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
        pick(options[active].id);
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
    title={`${label} — ${selected?.note || selected?.name}`}
    onclick={() => (open ? close() : openMenu())}
    onkeydown={onTriggerKeydown}
  >
    <span class="btn-select-label">{selected?.name ?? value}</span>
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

<!-- Ours rather than a native <select>, and fixed: see menu.ts. -->
{#if open && placement}
  <ul
    bind:this={listEl}
    class="popover menu"
    role="listbox"
    aria-label={label}
    aria-activedescendant={`${uid}-opt-${active}`}
    tabindex="-1"
    onkeydown={onListKeydown}
    style={menuStyle(placement)}
  >
    {#each options as option, i (option.id)}
      <!-- The listbox owns the keyboard: the rows aren't focusable, so a key
           handler on each one would never fire. -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <li
        id={`${uid}-opt-${i}`}
        role="option"
        aria-selected={option.id === value}
        data-active={i === active}
        class="menu-item row"
        class:on={option.id === value}
        onclick={() => pick(option.id)}
        onpointermove={() => {
          keyNav = false;
          active = i;
        }}
      >
        <span class="menu-tick" aria-hidden="true">{option.id === value ? "✓" : ""}</span>
        <span class="lines">
          <span class="menu-label">{option.name}</span>
          <!-- What the mode actually does, so the choice doesn't rest on the
               user remembering which word means which. -->
          {#if option.note}<span class="note">{option.note}</span>{/if}
        </span>
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

  /* Matches the model picker it sits beside: the spawn form's controls are one
     row of the same height and type size. */
  .trigger {
    width: 100%;
    font-size: var(--text-lg);
    padding: 0.5rem 0.7rem;
  }

  /* Nothing here says which mode is on but the label: this is the model
     picker's twin, and the pair reads as one strip only if the second control
     stays as quiet as the first. The menu's ticked row carries the choice. */

  /* A two-line row: the mode, and what it does under it. */
  .row {
    align-items: flex-start;
  }

  .lines {
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
    min-width: 0;
  }

  .note {
    color: var(--fg-muted);
    font-size: var(--text-xs);
  }

  /* Under the composer box this is a quiet secondary control rather than an
     input to fill in — `.btn-ghost` does the rest. */
  .compact .trigger {
    font-size: var(--text-xs);
    padding: 0.25rem 0.45rem;
  }
</style>
