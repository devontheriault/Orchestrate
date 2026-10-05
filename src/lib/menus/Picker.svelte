<script lang="ts">
  /**
   * One choice from a short list, each row saying what it means: the Mode a
   * Turn runs in, and the Host a new agent starts on (ADR 0011). The model
   * picker's twin, minus the submenu: nothing hangs off these rows. A row
   * that can't be picked right now is listed, and says why.
   */
  import { menuStyle, placeMenu } from "./menu";
  import { Listbox } from "./listbox.svelte";

  type Choice = { id: string; name: string; note?: string; disabled?: boolean };

  let {
    value,
    options,
    onpick,
    disabled = false,
    /** Sized for the composer strip rather than a dialog. */
    compact = false,
    label,
  }: {
    value: string;
    options: Choice[];
    onpick: (id: string) => void;
    disabled?: boolean;
    compact?: boolean;
    label: string;
  } = $props();

  const uid = $props.id();

  const selected = $derived(options.find((o) => o.id === value) ?? options[0]);
  const at = () => options.findIndex((o) => o.id === value);

  const box = new Listbox({
    count: () => options.length,
    // These live at the bottom of the window, so dropping upward is the common
    // case rather than the exception. Wider than the trigger: each row carries
    // a line saying what it means.
    place: (trigger) => placeMenu(trigger, { minWidth: 224, maxHeight: 280, align: "right" }),
    pick,
  });

  function pick(i: number) {
    const option = options[i];
    if (!option || option.disabled) return;
    onpick(option.id);
    box.close();
  }
</script>

<span class="picker" class:compact>
  <button
    bind:this={box.trigger}
    type="button"
    class="btn btn-select trigger"
    class:btn-ghost={compact}
    {disabled}
    aria-haspopup="listbox"
    aria-expanded={box.open}
    aria-label={label}
    title={`${label} — ${selected?.note || selected?.name}`}
    onclick={() => box.toggle(at())}
    onkeydown={(e) => box.triggerKey(e, at())}
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
{#if box.open && box.placement}
  <ul
    bind:this={box.list}
    class="popover menu"
    role="listbox"
    aria-label={label}
    aria-activedescendant={`${uid}-opt-${box.active}`}
    tabindex="-1"
    onkeydown={(e) => box.listKey(e)}
    style={menuStyle(box.placement)}
  >
    {#each options as option, i (option.id)}
      <!-- The listbox owns the keyboard: the rows aren't focusable, so a key
           handler on each one would never fire. -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <li
        id={`${uid}-opt-${i}`}
        role="option"
        aria-selected={option.id === value}
        aria-disabled={option.disabled}
        data-active={i === box.active}
        class="menu-item row"
        class:on={option.id === value}
        class:off={option.disabled}
        onclick={() => pick(i)}
        onpointermove={() => box.hover(i)}
      >
        <span class="menu-tick" aria-hidden="true">{option.id === value ? "✓" : ""}</span>
        <span class="lines">
          <span class="menu-label">{option.name}</span>
          <!-- What the choice actually does, so it doesn't rest on the user
               remembering which word means which. -->
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
     row of the same height and type size. Nothing says which choice is on but
     the label: the strip reads as one only if each control stays as quiet as
     the model picker. The menu's ticked row carries the choice. */
  .trigger {
    width: 100%;
    font-size: var(--text-lg);
    padding: 0.5rem 0.7rem;
  }

  /* A two-line row: the choice, and what it means under it. */
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

  .off {
    opacity: 0.5;
    cursor: default;
  }

  /* Under the composer box this is a quiet secondary control rather than an
     input to fill in — `.btn-ghost` does the rest. */
  .compact .trigger {
    font-size: var(--text-xs);
    padding: 0.25rem 0.45rem;
  }
</style>
