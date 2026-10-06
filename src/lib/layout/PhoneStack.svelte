<script module lang="ts">
  import type { SpaceId } from "$lib/spaces/space.svelte";

  /** Each Space's stack, for a way back that lives outside it: the bar's button, a tab tapped again. */
  const backs = new Map<SpaceId, () => boolean>();

  /**
   * Slide the screen open in `id`'s stack off to its list, as its own back
   * button would. False when there's no screen open there to leave, or no
   * stack there at all (not a phone).
   */
  export function goBack(id: SpaceId): boolean {
    return backs.get(id)?.() ?? false;
  }
</script>

<script lang="ts">
  /**
   * A phone's one-screen-at-a-time pair: a list, and the screen opened from
   * it pushed over the top, as iOS does it. The screen slides in from the
   * right and the list drifts left under it; a swipe to the right, from
   * anywhere on the screen, pulls it back off under the finger, and letting
   * go past halfway (or with a flick) finishes the job.
   *
   * The list stays mounted underneath, so going back finds it scrolled where
   * it was left.
   */
  import type { Snippet } from "svelte";
  import { untrack } from "svelte";
  import { Tween } from "svelte/motion";
  import { cubicOut } from "svelte/easing";
  import { goesThrough, reducedMotion, settleMs, swipe } from "./swipe";

  let {
    space: id,
    open,
    onclose,
    list,
    detail,
  }: {
    /** The Space it's in, for `goBack`. */
    space: SpaceId;
    /** Whether a screen is open over the list. */
    open: boolean;
    /** Close the screen: called once it has slid off, and waited for. */
    onclose: () => unknown;
    list: Snippet;
    detail: Snippet;
  } = $props();

  const PUSH_MS = 340;

  /** How far the screen has slid off to the right: 0 over the list, 1 gone. */
  const off = new Tween(untrack(() => (open ? 0 : 1)), { duration: PUSH_MS, easing: cubicOut });
  /** Whether the screen is drawn: from opening until it's slid off. */
  let shown = $state(untrack(() => open));
  /** A way back underway, which a second can't start over. */
  let leaving = $state(false);
  let width = $state(390);

  const ms = (n: number) => (reducedMotion() ? 0 : n);

  // Opened or closed from outside: by a tap on the list, or the open thing
  // going away (an agent deleted, a note removed). A screen closed that way has nothing left to
  // show, so it goes at once and the list slides back in on its own.
  $effect(() => {
    if (leaving) return;
    if (open && !shown) {
      shown = true;
      untrack(() => void off.set(0, { duration: ms(PUSH_MS) }));
    } else if (!open && shown) {
      shown = false;
      untrack(() => void off.set(1, { duration: ms(PUSH_MS) }));
    }
  });

  /** Slide the screen off from wherever it is, then close it. */
  function leave(duration: number) {
    leaving = true;
    void off.set(1, { duration: ms(duration) }).then(async () => {
      try {
        await onclose();
      } finally {
        shown = false;
        leaving = false;
      }
    });
  }

  function back(): boolean {
    if (leaving) return true;
    if (!open) return false;
    leave(PUSH_MS);
    return true;
  }

  $effect(() => {
    backs.set(id, back);
    return () => {
      if (backs.get(id) === back) backs.delete(id);
    };
  });

  const gesture = swipe({
    holds: () => open,
    can: (dir) => open && !leaving && dir === 1,
    move: (dx) => void off.set(Math.max(0, dx) / width, { duration: 0 }),
    end: (dx, velocity) => {
      const left = (1 - off.current) * width;
      if (goesThrough(dx, width, velocity)) leave(settleMs(left, velocity));
      else void off.set(0, { duration: ms(settleMs(off.current * width, 0)) });
    },
  });

  /** Moving at all: while still, neither layer carries a transform, which would trap fixed menus inside it. */
  const sliding = $derived(shown && off.current > 0 && off.current < 1);
</script>

<div class="phone-stack" bind:clientWidth={width} {@attach gesture}>
  <div
    class="layer list"
    inert={shown}
    style:visibility={shown && off.current === 0 ? "hidden" : undefined}
    style:--off={off.current}
    style:transform={off.current < 1 ? `translateX(${(off.current - 1) * 30}%)` : undefined}
  >
    {@render list()}
  </div>
  {#if shown}
    <div
      class="layer detail"
      class:sliding
      style:transform={off.current > 0 ? `translateX(${off.current * 100}%)` : undefined}
    >
      {@render detail()}
    </div>
  {/if}
</div>

<style>
  .phone-stack {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: grid;
    grid-template: minmax(0, 1fr) / minmax(0, 1fr);
    overflow: hidden;
  }

  .layer {
    grid-area: 1 / 1;
    min-width: 0;
    min-height: 0;
    display: flex;
  }

  /* Shaded the more the screen covers it; under a screen at rest it isn't
     painted at all. */
  .list::after {
    content: "";
    position: absolute;
    inset: 0;
    background: #000;
    opacity: calc((1 - var(--off, 1)) * 0.25);
    pointer-events: none;
  }

  .list {
    position: relative;
  }

  /* Filled while it moves, since a see-through theme's screen would show
     the list through it. */
  .detail.sliding {
    background: var(--float-bg, var(--surface));
    -webkit-backdrop-filter: var(--float-filter, none);
    backdrop-filter: var(--float-filter, none);
    box-shadow: -8px 0 24px rgb(0 0 0 / 0.18);
  }
</style>
