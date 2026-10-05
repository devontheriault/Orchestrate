<script lang="ts">
  /**
   * The window below its bar: the rail, and the current Space filling the rest
   * — or, on a phone, the Space over a tab bar.
   *
   * Every Space opened since launch stays mounted, stacked in one cell, and
   * the ones not showing are hidden and inert. Hidden rather than unmounted,
   * so nothing in them is lost: Agents comes back on the same Agent, scrolled
   * to the same place, with the same draft in its composer. Hidden with
   * `visibility` rather than `display`, so they keep their layout, and their
   * scroll positions with it.
   */
  import { store } from "$lib/state/store.svelte";
  import { viewport } from "$lib/layout/viewport.svelte";
  import { SPACES, currentSpaceCss, spaceForKey } from "./spaces";
  import { space, type SpaceId } from "./space.svelte";
  import SpaceRail from "./SpaceRail.svelte";

  space.start(SPACES.map((s) => s.id));

  // Ctrl/⌘ + 1–4. Unconditional, like the app's other Ctrl shortcuts: it types
  // nothing, so it's as good from inside the composer as from anywhere.
  $effect(() => {
    const onKey = (e: KeyboardEvent) => {
      const id = spaceForKey(e);
      if (!id) return;
      e.preventDefault();
      space.show(id);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  /**
   * Where the focus was in each Space when it was left, to put it back on the
   * way in: a hidden Space is inert, and that takes the focus out of it.
   */
  const focused = new Map<SpaceId, HTMLElement>();
  let boxes = $state<Partial<Record<SpaceId, HTMLElement>>>({});

  function noteFocus(id: SpaceId, e: FocusEvent) {
    if (e.target instanceof HTMLElement) focused.set(id, e.target);
  }

  $effect(() => {
    const el = focused.get(space.current);
    if (el?.isConnected && boxes[space.current]?.contains(el)) el.focus({ preventScroll: true });
  });

  /**
   * A phone's tab bar gives way to an open Agent, as iOS bars do to a pushed
   * screen: its composer wants the bottom edge, and the back button and the
   * swipe are the way out. Leaving the Agent brings the tabs back.
   */
  const tabs = $derived(
    viewport.phone && !(space.current === "agents" && (store.selectedAgent || store.drafting)),
  );
</script>

<svelte:head>
  {@html `<style>${currentSpaceCss()}</style>`}
</svelte:head>

<main class:phone={viewport.phone}>
  {#if !viewport.phone}<SpaceRail />{/if}

  <!-- Above the tab bar, the home indicator is the bar's to keep clear of. -->
  <div class="stack" class:over-tabs={tabs}>
    {#each SPACES as s (s.id)}
      <div
        class="box"
        data-space-box={s.id}
        inert={space.current !== s.id}
        bind:this={boxes[s.id]}
        onfocusin={(e) => noteFocus(s.id, e)}
      >
        {#if space.opened.has(s.id)}<s.component />{/if}
      </div>
    {/each}
  </div>

  {#if tabs}<SpaceRail phone />{/if}
</main>

<style>
  main {
    display: flex;
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }

  main.phone {
    flex-direction: column;
  }

  .stack {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: grid;
    grid-template: minmax(0, 1fr) / minmax(0, 1fr);
  }

  .stack.over-tabs {
    --safe-bottom: 0px;
  }

  /* All in the one cell; `currentSpaceCss` shows the current one. */
  .box {
    grid-area: 1 / 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    visibility: hidden;
  }
</style>
