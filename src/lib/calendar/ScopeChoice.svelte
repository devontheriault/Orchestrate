<script lang="ts">
  /**
   * "This event, or all of them?": asked before a change, a delete or an
   * answer to one occurrence of a repeating Calendar event, at the pointer.
   */
  import { dismissOnMove } from "$lib/menus/menu";
  import { calendar, type Asking } from "./calendar.svelte";

  let { asking }: { asking: Asking } = $props();

  let el: HTMLElement | undefined = $state();
  $effect(() => dismissOnMove(() => [el], () => calendar.choose(null)));
  $effect(() => {
    el?.querySelector<HTMLButtonElement>("button")?.focus();
  });

  const left = $derived(Math.max(8, Math.min(asking.at.x, (globalThis.innerWidth ?? 1000) - 260)));
  const top = $derived(Math.max(8, Math.min(asking.at.y - 8, (globalThis.innerHeight ?? 800) - 150)));
</script>

<div
  class="popover popover-above scope"
  bind:this={el}
  role="dialog"
  aria-label={`${asking.verb} which?`}
  tabindex="-1"
  style:left={`${left}px`}
  style:top={`${top}px`}
  onkeydown={(e) => {
    if (e.key === "Escape") {
      e.stopPropagation();
      calendar.choose(null);
    }
  }}
>
  <p>{asking.verb} <strong>{asking.event.title || "this"}</strong>: it repeats.</p>
  <div class="choices">
    <button class="btn" onclick={() => calendar.choose("this")}>Only this one</button>
    <button class="btn" onclick={() => calendar.choose("all")}>All of them</button>
  </div>
  <button class="btn btn-ghost btn-sm cancel" onclick={() => calendar.choose(null)}>Cancel</button>
</div>

<style>
  .scope {
    width: 15.5rem;
    padding: var(--space-5);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    font-size: var(--text-sm);
  }

  p {
    margin: 0;
    line-height: var(--leading-snug);
    overflow-wrap: anywhere;
  }

  .choices {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .choices .btn {
    justify-content: flex-start;
  }

  .cancel {
    align-self: flex-end;
  }
</style>
