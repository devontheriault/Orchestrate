<script lang="ts">
  /**
   * The Hosts this window talks to: this machine's, and any other of the
   * user's machines added by its Tailscale name (ADR 0012). Adding one is all
   * it takes — that machine's Host lets this window in if it's the same
   * Tailscale user. Removing one only stops this window listening: the agents
   * there are that machine's, and carry on.
   *
   * The name box is a combobox: it drops down the user's machines Tailscale
   * knows of, and still takes a name typed by hand for one it doesn't list.
   */
  import { hosts } from "$lib/state/hosts.svelte";
  import { store } from "$lib/state/store.svelte";
  import { api, type HostStatus, type Machine } from "$lib/api";
  import { dismissOnMove, menuStyle, placeMenu, stepActive, type Placement } from "$lib/menus/menu";
  import { machineChoices, machineNote } from "./machines";

  let { onclose }: { onclose: () => void } = $props();

  const uid = $props.id();

  let name = $state("");
  let adding = $state(false);
  let error = $state<string | null>(null);

  let machines = $state<Machine[]>([]);
  api.tailnetMachines().then(
    (m) => (machines = m),
    () => {},
  );

  const choices = $derived(
    machineChoices(
      machines,
      hosts.list.map((h) => h.id),
      name,
    ),
  );

  let open = $state(false);
  /** The row the arrows are on, or -1 for none: Enter then adds what's typed. */
  let active = $state(-1);
  let placement = $state<Placement | null>(null);
  let listEl: HTMLElement | undefined = $state();
  let boxEl: HTMLElement | undefined = $state();

  let inputEl: HTMLInputElement | undefined = $state();
  $effect(() => inputEl?.focus());

  /** Where the About note sits while the info icon is hovered or focused. */
  let tipAt = $state<Placement | null>(null);
  let infoEl: HTMLElement | undefined = $state();
  const showTip = () => {
    if (infoEl) tipAt = placeMenu(infoEl, { minWidth: 320, maxHeight: 320, align: "left" });
  };
  const hideTip = () => (tipAt = null);

  function openMenu() {
    if (!boxEl || !machines.length) return;
    placement = placeMenu(boxEl, { minWidth: 240, maxHeight: 240, align: "left" });
    open = true;
  }

  function closeMenu() {
    open = false;
    active = -1;
  }

  $effect(() => {
    if (!open) return;
    return dismissOnMove(() => [listEl, boxEl], closeMenu);
  });

  // What's typed narrows the list; the highlight starts over with it.
  $effect(() => {
    choices;
    active = -1;
  });

  $effect(() => {
    if (active < 0) return;
    listEl?.querySelector<HTMLElement>('[data-active="true"]')?.scrollIntoView({ block: "nearest" });
  });

  async function addNamed(n: string) {
    if (!n.trim() || adding) return;
    closeMenu();
    adding = true;
    error = await hosts.add(n);
    adding = false;
    if (!error) name = "";
  }

  function add(e: SubmitEvent) {
    e.preventDefault();
    const picked = open ? choices[active] : undefined;
    addNamed(picked ? picked.name : name);
  }

  function onInputKeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && open) {
      // Just the list: the dialog stays.
      e.preventDefault();
      e.stopPropagation();
      closeMenu();
      return;
    }
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    if (!open) {
      openMenu();
      return;
    }
    if (choices.length) active = stepActive(e.key, active, choices.length) ?? active;
  }

  async function remove(id: string) {
    error = await hosts.remove(id);
    // Its agents leave the window with it.
    if (!error) store.refresh();
  }

  function dot(s: HostStatus): string {
    switch (s.state) {
      case "connected":
        return "completed";
      case "connecting":
        return "stopped";
      case "updating":
        return "orphaned";
      default:
        return "failed";
    }
  }

  $effect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onclose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });
</script>

<!-- Closes on a click outside the dialog, not on one that lands inside it. -->
<div
  class="backdrop"
  role="presentation"
  onclick={(e) => {
    if (e.target === e.currentTarget) onclose();
  }}
>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="hosts-title">
    <header>
      <h2 id="hosts-title">Hosts</h2>
      <!-- The how and why, kept out of the way until asked for. -->
      <span
        class="info"
        bind:this={infoEl}
        role="presentation"
        onpointerenter={showTip}
        onpointerleave={hideTip}
        onfocusin={showTip}
        onfocusout={hideTip}
      >
        <button
          type="button"
          class="info-btn"
          aria-label="About Hosts"
          aria-describedby={`${uid}-about`}
        >
          <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
            <circle cx="8" cy="8" r="6.75" fill="none" stroke="currentColor" stroke-width="1.5" />
            <path d="M8 7.25v4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
            <circle cx="8" cy="4.9" r="0.9" fill="currentColor" />
          </svg>
        </button>
        <!-- Fixed, like the menus, so the dialog's scrolling can't clip it. -->
        <span
          class="popover tip"
          class:shown={tipAt}
          role="tooltip"
          id={`${uid}-about`}
          style={tipAt ? menuStyle(tipAt) : undefined}
        >
          <span>
            Each of your machines with this app runs a Host, which keeps its agents working whether
            or not a window is open. Add another machine to see and start agents on it from here.
          </span>
          <span>
            The machine needs this app installed, and Tailscale running and logged in as you. Only
            your own Tailscale user's machines are let in.
          </span>
        </span>
      </span>
    </header>

    <ul class="list">
      {#each hosts.list as h (h.id)}
        <li>
          <span class={`status-dot status-${dot(h.status)}`}></span>
          <span class="who">
            <span class="name">{hosts.label(h.id)}</span>
            <span class="state">
              {#if h.local}
                this machine
              {:else}
                {h.id}{#if hosts.problem(h.id)}&nbsp;· {hosts.problem(h.id)}{/if}
              {/if}
              {#if h.status.state === "refused"}— {h.status.reason}{/if}
            </span>
          </span>
          {#if !h.local}
            <button class="btn btn-ghost btn-sm" onclick={() => remove(h.id)}>Remove</button>
          {/if}
        </li>
      {/each}
    </ul>

    <form onsubmit={add}>
      <div class="box" bind:this={boxEl}>
        <input
          bind:this={inputEl}
          bind:value={name}
          placeholder={machines.length
            ? "Pick a machine"
            : "Tailscale name, like desktop"}
          aria-label="Tailscale name of the machine to add"
          role="combobox"
          aria-autocomplete="list"
          aria-expanded={open}
          aria-controls={`${uid}-machines`}
          aria-activedescendant={open && active >= 0 ? `${uid}-m-${active}` : undefined}
          spellcheck="false"
          autocomplete="off"
          onclick={openMenu}
          oninput={openMenu}
          onkeydown={onInputKeydown}
        />
        {#if machines.length}
          <button
            type="button"
            class="chevron"
            tabindex="-1"
            aria-label="Your machines"
            onclick={() => {
              if (open) closeMenu();
              else openMenu();
              inputEl?.focus();
            }}
          >
            <svg viewBox="0 0 10 6" width="10" height="6" aria-hidden="true">
              <path
                d="M1 1l4 4 4-4"
                fill="none"
                stroke="currentColor"
                stroke-width="1.5"
                stroke-linecap="round"
                stroke-linejoin="round"
              />
            </svg>
          </button>
        {/if}
      </div>
      <button class="btn btn-primary" disabled={!name.trim() || adding}>
        {adding ? "Adding…" : "Add"}
      </button>
    </form>
    <!-- Ours rather than a native <select>, and fixed: see menu.ts. -->
    {#if open && placement}
      <!-- Pressing a row mustn't take focus from the box: it keeps the keyboard. -->
      <div
        class="popover"
        style={menuStyle(placement)}
        onpointerdown={(e) => e.preventDefault()}
        role="presentation"
      >
        {#if choices.length}
          <ul
            bind:this={listEl}
            id={`${uid}-machines`}
            class="menu"
            role="listbox"
            aria-label="Your machines"
          >
            {#each choices as m, i (m.name)}
              <!-- The box owns the keyboard, as a combobox's input does. -->
              <!-- svelte-ignore a11y_click_events_have_key_events -->
              <li
                id={`${uid}-m-${i}`}
                role="option"
                aria-selected={i === active}
                data-active={i === active}
                class="menu-item machine"
                class:offline={!m.online}
                onclick={() => addNamed(m.name)}
                onpointermove={() => (active = i)}
              >
                <span class={`status-dot status-${m.online ? "completed" : "failed"}`}></span>
                <span class="lines">
                  <span class="menu-label">{m.name}</span>
                  <span class="note">{machineNote(m)}</span>
                </span>
              </li>
            {/each}
          </ul>
        {:else}
          <p class="empty">
            {name.trim()
              ? "None of your machines is called that — Add tries it anyway."
              : "Every machine Tailscale lists is already here."}
          </p>
        {/if}
      </div>
    {/if}
    {#if error}<p class="error">{error}</p>{/if}

    <footer>
      <button class="btn" onclick={onclose}>Done</button>
    </footer>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: var(--scrim);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: var(--space-6);
    z-index: var(--z-overlay);
    backdrop-filter: blur(2px);
  }

  .dialog {
    background: var(--float-bg, var(--surface));
    -webkit-backdrop-filter: var(--float-filter, none);
    backdrop-filter: var(--float-filter, none);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    width: min(30rem, 100%);
    max-height: 100%;
    overflow-y: auto;
    box-shadow: var(--shadow-modal);
    padding: 1.1rem 1.25rem 1rem;
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  h2 {
    font-size: var(--text-2xl);
    font-weight: var(--weight-semibold);
    margin: 0;
  }

  header {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  .info {
    display: inline-flex;
  }

  .info-btn {
    display: flex;
    padding: 0.2rem;
    border: none;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--fg-muted);
    cursor: help;
  }

  .info-btn:hover,
  .info-btn:focus-visible {
    color: var(--fg);
  }

  .tip {
    padding: 0.6rem 0.75rem;
    display: none;
    flex-direction: column;
    gap: var(--space-3);
    font-size: var(--text-sm);
    line-height: var(--leading-normal);
    color: var(--fg);
  }

  .tip.shown {
    display: flex;
  }

  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--panel-bg);
  }

  .list li {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    padding: var(--space-3) var(--space-5);
  }

  .list li + li {
    border-top: 1px solid var(--border);
  }

  .who {
    display: flex;
    flex-direction: column;
    min-width: 0;
    flex: 1;
  }

  .name {
    font-size: var(--text-md);
    font-weight: var(--weight-semibold);
  }

  .state {
    font-size: var(--text-xs);
    color: var(--fg-muted);
    overflow-wrap: anywhere;
  }

  form {
    display: flex;
    gap: var(--space-3);
  }

  .box {
    position: relative;
    flex: 1;
    min-width: 0;
    display: flex;
  }

  .chevron {
    position: absolute;
    right: 0;
    top: 0;
    bottom: 0;
    width: 2rem;
    display: flex;
    align-items: center;
    justify-content: center;
    border: none;
    background: none;
    color: var(--fg-muted);
    cursor: pointer;
  }

  .chevron:hover {
    color: var(--fg);
  }

  .machine {
    align-items: flex-start;
  }

  .machine .status-dot {
    margin-top: 0.4rem;
  }

  .offline {
    opacity: 0.6;
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

  .empty {
    margin: 0;
    padding: 0.7rem 0.85rem;
    color: var(--fg-muted);
    font-size: var(--text-sm);
  }

  input {
    flex: 1;
    min-width: 0;
    font: inherit;
    font-size: var(--text-md);
    padding: 0.45rem 0.6rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--panel-bg);
    color: var(--fg);
  }

  .box:has(.chevron) input {
    padding-right: 2rem;
  }

  input:focus {
    outline: none;
    border-color: var(--accent);
  }

  .error {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--danger-fg);
  }

  footer {
    display: flex;
    justify-content: flex-end;
  }
</style>
