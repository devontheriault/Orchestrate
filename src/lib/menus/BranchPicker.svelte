<script lang="ts">
  /**
   * The merge target, picked from the project's branches. Same control as the
   * model picker — trigger, menu, keys — with no submenu hanging off the rows:
   * a branch has nothing to qualify.
   */
  import { menuStyle, placeMenu } from "./menu";
  import { Listbox } from "./listbox.svelte";

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

  const box = new Listbox({
    count: () => options.length,
    // The merge box sits low in a scrolling pane, so dropping upward is as
    // likely as down. Right edges line up: that's the edge the trigger is
    // aligned on.
    place: (trigger) => placeMenu(trigger, { minWidth: 176, maxHeight: 280, align: "right" }),
    pick: (i) => pick(options[i]),
  });

  function pick(branch: string) {
    value = branch;
    box.close();
  }
</script>

<span class="picker">
  <button
    bind:this={box.trigger}
    type="button"
    class="btn btn-select trigger"
    disabled={disabled || options.length === 0}
    aria-haspopup="listbox"
    aria-expanded={box.open}
    aria-label={label}
    title={value ? `${label} — ${value}` : label}
    onclick={() => box.toggle(options.indexOf(value))}
    onkeydown={(e) => box.triggerKey(e, options.indexOf(value))}
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
    {#each options as name, i (name)}
      <!-- The listbox owns the keyboard: the rows aren't focusable, so a key
           handler on each one would never fire. -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <li
        id={`${uid}-opt-${i}`}
        role="option"
        aria-selected={name === value}
        data-active={i === box.active}
        class="menu-item"
        class:on={name === value}
        onclick={() => pick(name)}
        onpointermove={() => box.hover(i)}
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
