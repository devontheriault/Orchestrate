<script lang="ts">
  import { DEFAULT_MODEL } from "./api";
  import { store } from "./store.svelte";

  let {
    value = $bindable(DEFAULT_MODEL),
    disabled = false,
    /** Sized for the composer strip rather than a dialog. */
    compact = false,
    label = "Model",
  }: {
    value?: string;
    disabled?: boolean;
    compact?: boolean;
    label?: string;
  } = $props();

  // A value the account no longer offers still gets an entry, so an agent that
  // ran on a retired model shows what it ran on instead of silently reading as
  // something else.
  const options = $derived.by(() => {
    const listed = store.models.map((m) => ({ id: m.id, name: m.display_name }));
    const known = value === DEFAULT_MODEL || listed.some((o) => o.id === value);
    return known ? listed : [...listed, { id: value, name: value }];
  });
</script>

<span class="picker" class:compact>
  <select bind:value {disabled} aria-label={label} title={label}>
    <option value={DEFAULT_MODEL}>Default</option>
    {#each options as option (option.id)}
      <option value={option.id}>{option.name}</option>
    {/each}
  </select>
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
</span>

<style>
  .picker {
    position: relative;
    display: inline-flex;
    align-items: center;
    min-width: 0;
  }

  /*
   * `appearance: none` is the whole point: left native, the control is drawn by
   * the platform in the desktop theme's colours, which have nothing to do with
   * this page and read as a foreign element pasted into it. Everything below
   * restates the same surface, border, and radius the inputs beside it use.
   */
  select {
    appearance: none;
    width: 100%;
    min-width: 0;
    font-family: inherit;
    font-size: 0.88rem;
    line-height: 1.4;
    color: var(--fg);
    background: var(--panel-bg);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 0.5rem 1.9rem 0.5rem 0.7rem;
    cursor: pointer;
    text-overflow: ellipsis;
  }

  select:hover:not(:disabled) {
    border-color: var(--accent);
  }

  select:focus-visible {
    outline: none;
    border-color: var(--accent);
    box-shadow: 0 0 0 3px rgba(59, 130, 246, 0.15);
  }

  select:disabled {
    opacity: 0.55;
    cursor: not-allowed;
  }

  /*
   * The open dropdown is an OS popup, so this is as far as styling reaches on
   * some platforms — worth setting for the ones where it lands.
   */
  option {
    background: var(--surface);
    color: var(--fg);
  }

  .chevron {
    position: absolute;
    right: 0.65rem;
    display: flex;
    /* Drawn in currentColor, so it follows the theme instead of needing a
       second copy of the icon for dark mode. */
    color: var(--fg-muted);
    pointer-events: none;
  }

  /* The composer strip has a send button to sit beside; a long model name
     ellipses rather than pushing it off a narrow pane. */
  .compact {
    max-width: 9rem;
  }

  .compact select {
    font-size: 0.78rem;
    padding: 0.38rem 1.55rem 0.38rem 0.55rem;
  }

  .compact .chevron {
    right: 0.5rem;
  }
</style>
