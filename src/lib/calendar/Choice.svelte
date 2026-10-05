<script lang="ts" generics="T extends string">
  /**
   * One of a few choices, the way every picker in the app works (see
   * `menus/menu.ts`): a trigger, a list under it, the arrows to walk it.
   * It lives in the editor, itself a popover, so its list floats above it.
   */
  import { dismissOnMove, menuStyle, opensMenu, placeMenu, stepActive, type Placement } from "$lib/menus/menu";

  type Option = { value: T; label: string; color?: string | null; note?: string };

  let {
    value = $bindable(),
    options,
    label,
  }: { value: T; options: Option[]; label: string } = $props();

  const uid = $props.id();
  let open = $state(false);
  let active = $state(0);
  let triggerEl: HTMLButtonElement | undefined = $state();
  let listEl: HTMLElement | undefined = $state();
  let placement = $state<Placement | null>(null);

  const current = $derived(options.find((o) => o.value === value));

  function openMenu() {
    if (!triggerEl || !options.length) return;
    active = Math.max(0, options.findIndex((o) => o.value === value));
    placement = placeMenu(triggerEl, { minWidth: 220, maxHeight: 300, align: "left" });
    open = true;
  }

  function close(refocus = true) {
    if (!open) return;
    open = false;
    if (refocus) triggerEl?.focus();
  }

  function pick(v: T) {
    value = v;
    close();
  }

  $effect(() => {
    if (!open) return;
    return dismissOnMove(() => [listEl, triggerEl], () => close(false));
  });

  $effect(() => {
    if (open) listEl?.focus();
  });

  function onListKeydown(e: KeyboardEvent) {
    const next = stepActive(e.key, active, options.length);
    if (next !== null) {
      e.preventDefault();
      active = next;
      return;
    }
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      close();
    } else if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      pick(options[active].value);
    } else if (e.key === "Tab") {
      close(false);
    }
  }
</script>

<button
  bind:this={triggerEl}
  type="button"
  class="btn btn-select trigger"
  aria-haspopup="listbox"
  aria-expanded={open}
  aria-label={label}
  onclick={() => (open ? close() : openMenu())}
  onkeydown={(e) => {
    if (opensMenu(e.key)) {
      e.preventDefault();
      openMenu();
    }
  }}
>
  {#if current?.color !== undefined}<span class="dot" style:--c={current?.color}></span>{/if}
  <span class="btn-select-label">{current?.label ?? label}</span>
  <svg class="btn-select-chevron" viewBox="0 0 10 6" width="10" height="6" aria-hidden="true">
    <path d="M1 1l4 4 4-4" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" />
  </svg>
</button>

{#if open && placement}
  <ul
    bind:this={listEl}
    class="popover popover-above menu"
    role="listbox"
    aria-label={label}
    aria-activedescendant={`${uid}-opt-${active}`}
    tabindex="-1"
    onkeydown={onListKeydown}
    style={menuStyle(placement)}
  >
    {#each options as o, i (o.value)}
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <li
        id={`${uid}-opt-${i}`}
        role="option"
        aria-selected={o.value === value}
        data-active={i === active}
        class="menu-item"
        class:on={o.value === value}
        onclick={() => pick(o.value)}
        onpointermove={() => (active = i)}
      >
        {#if o.color !== undefined}
          <span class="dot" style:--c={o.color}></span>
        {:else}
          <span class="menu-tick" aria-hidden="true">{o.value === value ? "✓" : ""}</span>
        {/if}
        <span class="menu-label">{o.label}</span>
        {#if o.note}<span class="menu-note">{o.note}</span>{/if}
      </li>
    {/each}
  </ul>
{/if}

<style>
  .trigger {
    width: 100%;
    min-width: 0;
  }

  .dot {
    flex: none;
    width: 0.6rem;
    height: 0.6rem;
    border-radius: 50%;
    background: var(--c, var(--accent));
  }
</style>
