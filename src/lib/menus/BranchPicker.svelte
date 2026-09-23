<script lang="ts">
  /**
   * The merge target, picked from the project's branches. Same control as the
   * model picker — trigger, menu, keys — with no submenu hanging off the rows:
   * a branch has nothing to qualify.
   */
  import { dismissOnMove, menuStyle, opensMenu, placeMenu, stepActive, type Placement } from "./menu";

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

  let placement = $state<Placement | null>(null);

  function place() {
    if (!triggerEl) return;
    // The merge box sits low in a scrolling pane, so dropping upward is as
    // likely as down. Right edges line up: that's the edge the trigger is
    // aligned on.
    placement = placeMenu(triggerEl, { minWidth: 176, maxHeight: 280, align: "right" });
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
        pick(options[active]);
        break;
    }
  }
</script>

<span class="picker">
  <button
    bind:this={triggerEl}
    type="button"
    class="btn btn-select trigger"
    disabled={disabled || options.length === 0}
    aria-haspopup="listbox"
    aria-expanded={open}
    aria-label={label}
    title={value ? `${label} — ${value}` : label}
    onclick={() => (open ? close() : openMenu())}
    onkeydown={onTriggerKeydown}
  >
    <span class="btn-select-label">{value || "No branches"}</span>
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
    {#each options as name, i (name)}
      <!-- The listbox owns the keyboard: the rows aren't focusable, so a key
           handler on each one would never fire. -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <li
        id={`${uid}-opt-${i}`}
        role="option"
        aria-selected={name === value}
        data-active={i === active}
        class="menu-item"
        class:on={name === value}
        onclick={() => pick(name)}
        onpointermove={() => {
          keyNav = false;
          active = i;
        }}
      >
        <span class="menu-tick" aria-hidden="true">{name === value ? "✓" : ""}</span>
        <span class="menu-label">{name}</span>
        <!-- Which branch the project itself is on: the one where a merge shows
             up in the user's own checkout right away. -->
        {#if name === current}<span class="menu-note">checked out</span>{/if}
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

  /* The app's select trigger, capped for the merge row it sits in. */
  .trigger {
    max-width: 14rem;
  }
</style>
