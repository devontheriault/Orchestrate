<script lang="ts" generics="T extends string">
  /**
   * One of a few choices, the way every picker in the app works (see
   * `menus/menu.ts`): a trigger, a list under it, the arrows to walk it.
   * It lives in the editor, itself a popover, so its list floats above it.
   */
  import { menuStyle, placeMenu } from "$lib/menus/menu";
  import { Listbox } from "$lib/menus/listbox.svelte";

  type Option = { value: T; label: string; color?: string | null; note?: string };

  let {
    value = $bindable(),
    options,
    label,
  }: { value: T; options: Option[]; label: string } = $props();

  const uid = $props.id();

  const current = $derived(options.find((o) => o.value === value));
  const at = () => options.findIndex((o) => o.value === value);

  const box = new Listbox({
    count: () => options.length,
    place: (trigger) => placeMenu(trigger, { minWidth: 220, maxHeight: 300, align: "left" }),
    pick: (i) => pick(options[i].value),
  });

  function pick(v: T) {
    value = v;
    box.close();
  }
</script>

<button
  bind:this={box.trigger}
  type="button"
  class="btn btn-select trigger"
  aria-haspopup="listbox"
  aria-expanded={box.open}
  aria-label={label}
  onclick={() => box.toggle(at())}
  onkeydown={(e) => box.triggerKey(e, at())}
>
  {#if current?.color !== undefined}<span class="dot" style:--c={current?.color}></span>{/if}
  <span class="btn-select-label">{current?.label ?? label}</span>
  <svg class="btn-select-chevron" viewBox="0 0 10 6" width="10" height="6" aria-hidden="true">
    <path d="M1 1l4 4 4-4" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" />
  </svg>
</button>

{#if box.open && box.placement}
  <ul
    bind:this={box.list}
    class="popover popover-above menu"
    role="listbox"
    aria-label={label}
    aria-activedescendant={`${uid}-opt-${box.active}`}
    tabindex="-1"
    onkeydown={(e) => box.listKey(e)}
    style={menuStyle(box.placement)}
  >
    {#each options as o, i (o.value)}
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <li
        id={`${uid}-opt-${i}`}
        role="option"
        aria-selected={o.value === value}
        data-active={i === box.active}
        class="menu-item"
        class:on={o.value === value}
        onclick={() => pick(o.value)}
        onpointermove={() => box.hover(i)}
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
