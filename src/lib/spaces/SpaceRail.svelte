<script lang="ts">
  /**
   * The rail: one icon per Space, down the window's left edge, with Settings
   * at its foot so they're in reach from every Space. On a phone it's a tab
   * bar along the bottom instead, icons over labels, above the home indicator.
   *
   * Which tab is lit comes from `<html data-space>` through the rules in
   * `currentSpaceCss`, not from the markup, so the first frame lights the
   * right one before the app has started (see `spaces.ts`).
   */
  import SettingsMenu from "$lib/sidebar/SettingsMenu.svelte";
  import { SPACES } from "./spaces";
  import { space } from "./space.svelte";
  import SpaceIcon from "./SpaceIcon.svelte";
  import { goBack } from "$lib/layout/PhoneStack.svelte";

  let { phone = false }: { phone?: boolean } = $props();
</script>

<nav class="rail" class:phone aria-label="Spaces">
  <ul>
    {#each SPACES as s (s.id)}
      <li>
        <button
          class="tab"
          data-space-tab={s.id}
          aria-current={space.current === s.id ? "page" : undefined}
          aria-label={s.label}
          aria-keyshortcuts={`Control+${s.shortcut} Meta+${s.shortcut}`}
          onclick={() => {
            // Tapped again, a tab goes back to its Space's top, as iOS's do.
            if (phone && space.current === s.id) goBack(s.id);
            else space.show(s.id);
          }}
        >
          <span class="glyph"><SpaceIcon icon={s.icon} /></span>
          {#if phone}
            <span class="label">{s.label}</span>
          {:else}
            <!-- The tooltip: the name, and the keys that come here. -->
            <span class="tip" aria-hidden="true">
              {s.label}
              <kbd><span class="ctrl">Ctrl</span><span class="cmd">⌘</span>{s.shortcut}</kbd>
            </span>
          {/if}
        </button>
      </li>
    {/each}
  </ul>

  {#if !phone}
    <div class="foot">
      <SettingsMenu collapsed />
    </div>
  {/if}
</nav>

<style>
  .rail {
    flex: none;
    width: var(--spaces-rail);
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: var(--space-3) 0;
    /* No rule to its right: it's painted like the sidebar beside it and the
       title bar's lead segment above, so the three read as one panel. */
    background: var(--panel-bg);
    user-select: none;
    -webkit-user-select: none;
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .tab {
    /* 1 on the current Space's tab, from `currentSpaceCss`; 0 on the rest. */
    --on: var(--current, 0);
    position: relative;
    width: 2.4rem;
    height: 2.4rem;
    display: grid;
    place-items: center;
    padding: 0;
    border: none;
    border-radius: var(--radius-lg);
    background: color-mix(in srgb, var(--accent) calc(var(--on) * 15%), transparent);
    color: color-mix(in srgb, var(--accent) calc(var(--on) * 100%), var(--fg-muted));
    font-size: 1.2rem;
    cursor: pointer;
    transition:
      background var(--transition-fast),
      color var(--transition-fast);
  }

  .tab:hover {
    background: color-mix(in srgb, var(--accent) calc(var(--on) * 15%), var(--hover));
    color: color-mix(in srgb, var(--accent) calc(var(--on) * 100%), var(--fg));
  }

  .tab:focus-visible {
    outline: none;
    box-shadow: var(--focus-ring);
  }

  /* The current tab's marker, on the rail's left edge: a pill that grows in
     from nothing as the tab lights. */
  .tab::before {
    content: "";
    position: absolute;
    left: calc((var(--spaces-rail) - 2.4rem) / -2);
    top: 50%;
    width: 3px;
    height: 1.25rem;
    border-radius: 0 var(--radius-pill) var(--radius-pill) 0;
    background: var(--accent);
    transform: translateY(-50%) scaleY(var(--on));
    transition: transform var(--duration-base) var(--ease);
  }

  .glyph {
    display: grid;
    place-items: center;
  }

  /* A label beside the tab, the way a native tooltip would sit, but at once
     and in the app's own type. */
  .tip {
    position: absolute;
    left: calc(100% + var(--space-4));
    top: 50%;
    z-index: var(--z-popover);
    display: flex;
    align-items: center;
    gap: var(--space-4);
    padding: var(--space-2) var(--space-3) var(--space-2) var(--space-4);
    border: var(--border-width) solid var(--border);
    border-radius: var(--radius-md);
    background: var(--float-bg, var(--surface));
    -webkit-backdrop-filter: var(--float-filter, none);
    backdrop-filter: var(--float-filter, none);
    box-shadow: var(--shadow-sm);
    color: var(--fg);
    font-size: var(--text-sm);
    font-weight: var(--weight-medium);
    white-space: nowrap;
    pointer-events: none;
    opacity: 0;
    transform: translate(-0.25rem, -50%);
    transition:
      opacity var(--transition-fast),
      transform var(--transition-fast);
  }

  /* A hover has to settle first, so sweeping down the rail doesn't flicker a
     label past each icon; a keyboard focus shows it at once. */
  .tab:hover .tip {
    transition-delay: 0.35s;
  }

  .tab:hover .tip,
  .tab:focus-visible .tip {
    opacity: 1;
    transform: translate(0, -50%);
  }

  kbd {
    padding: 0 var(--space-2);
    border: var(--border-width) solid var(--border);
    border-radius: var(--radius-xs);
    font-size: var(--text-2xs);
    color: var(--fg-muted);
  }

  /* ⌘ on a Mac, Ctrl everywhere else. Keyed off the frame `app.html` wrote,
     so it's right on the first frame. */
  .cmd,
  :global(html[data-frame="mac"]) .ctrl {
    display: none;
  }

  :global(html[data-frame="mac"]) .cmd {
    display: inline;
  }

  .foot {
    margin-top: auto;
    width: 2.4rem;
  }

  /* Settings as one more square at the rail's foot, its icon sized like a
     secondary control's: a step down from the Spaces above it. */
  .foot :global(.settings) {
    width: 2.4rem;
    height: 2.4rem;
    justify-content: center;
    padding: 0;
    border-radius: var(--radius-lg);
  }

  .foot :global(.settings svg) {
    width: 1rem;
    height: 1rem;
  }

  /* ------------------------------------------------------------ phone */

  .rail.phone {
    width: auto;
    flex-direction: row;
    padding: 0 var(--space-3) var(--safe-bottom);
    border-top: var(--border-width) solid var(--border);
  }

  .phone ul {
    flex: 1;
    flex-direction: row;
    gap: 0;
  }

  .phone li {
    flex: 1;
  }

  /* A full-width target per tab, a thumb's height, and no box: on a phone
     the lit icon and label are the whole selected state, as iOS draws it. */
  .phone .tab {
    width: 100%;
    height: 3.1rem;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 0.2rem;
    border-radius: 0;
    background: transparent;
    font-size: 1.35rem;
    -webkit-tap-highlight-color: transparent;
  }

  .phone .tab::before {
    display: none;
  }

  .label {
    font-size: var(--text-3xs);
    font-weight: var(--weight-medium);
    letter-spacing: 0.01em;
  }

  /* The keyboard is up only over something being typed in, and the tabs
     would ride up on top of it. */
  :global(html[data-keyboard]) .rail.phone {
    display: none;
  }
</style>
