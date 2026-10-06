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
   *
   * On a phone, a swipe sideways over a Space's top screen pulls the next one
   * in beside it, the way the tabs below are laid out.
   */
  import { Tween } from "svelte/motion";
  import { cubicOut } from "svelte/easing";
  import { goesThrough, reducedMotion, resist, settleMs, swipe, type Dir } from "$lib/layout/swipe";
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

  /** A swipe between Spaces under way: the Space coming in, if there's one that way. */
  let pull = $state<{ to: SpaceId | null; dir: Dir } | null>(null);
  /** How far the Space showing has moved across. */
  const across = new Tween(0, { easing: cubicOut });
  /** Letting go: finishing the swipe or putting it back, which a new one waits for. */
  let settling = false;
  let width = $state(390);

  /** The Space beside the one showing: swiping left brings in the next tab. */
  function beside(dir: Dir): SpaceId | null {
    const i = SPACES.findIndex((s) => s.id === space.current);
    return SPACES[i - dir]?.id ?? null;
  }

  const pager = swipe({
    can: () => tabs && !settling,
    move: (dx) => {
      const dir: Dir = dx < 0 ? -1 : 1;
      const to = beside(dir);
      if (to) space.opened.add(to);
      pull = { to, dir };
      void across.set(to ? dx : resist(dx), { duration: 0 });
    },
    end: (dx, velocity) => {
      const to = pull?.to;
      const motion = (n: number) => (reducedMotion() ? 0 : n);
      settling = true;
      if (to && pull && goesThrough(dx, width, velocity)) {
        const goal = pull.dir * width;
        void across.set(goal, { duration: motion(settleMs(goal - dx, velocity)) }).then(() => {
          space.show(to);
          pull = null;
          void across.set(0, { duration: 0 });
          settling = false;
        });
      } else {
        void across.set(0, { duration: motion(settleMs(across.current, 0)) }).then(() => {
          pull = null;
          settling = false;
        });
      }
    },
  });

  /** Where a Space's box is drawn during a swipe: the one showing, and the one coming in beside it. */
  function shift(id: SpaceId): string | undefined {
    if (!pull) return undefined;
    if (id === space.current) return `translateX(${across.current}px)`;
    if (id === pull.to) return `translateX(${across.current - pull.dir * width}px)`;
    return undefined;
  }
</script>

<svelte:head>
  {@html `<style>${currentSpaceCss()}</style>`}
</svelte:head>

<main class:phone={viewport.phone}>
  {#if !viewport.phone}<SpaceRail />{/if}

  <!-- Above the tab bar, the home indicator is the bar's to keep clear of. -->
  <div class="stack" class:over-tabs={tabs} bind:clientWidth={width} {@attach pager}>
    {#each SPACES as s (s.id)}
      <div
        class="box"
        data-space-box={s.id}
        style:transform={shift(s.id)}
        style:visibility={pull?.to === s.id ? "visible" : undefined}
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
