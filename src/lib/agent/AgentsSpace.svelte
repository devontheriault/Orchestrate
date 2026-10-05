<script lang="ts">
  /**
   * The Agents Space: the project list and the agent opened from it, side by
   * side, or one screen at a time on a phone.
   */
  import { fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { isBackSwipe, type Point } from "$lib/layout/backSwipe";
  import ProjectSidebar from "$lib/sidebar/ProjectSidebar.svelte";
  import AgentPane from "./AgentPane.svelte";
  import PaneDivider from "$lib/layout/PaneDivider.svelte";
  import { store } from "$lib/state/store.svelte";
  import { usage } from "$lib/usage/usage.svelte";
  import { viewport } from "$lib/layout/viewport.svelte";
  import { panes, MIN_DETAIL, MIN_PROJECTS_DRAG } from "$lib/layout/panes.svelte";
  import { space } from "$lib/spaces/space.svelte";

  // Global "n" opens the blank page for a new agent, if a project is selected,
  // this Space is the one showing, and the user is not typing in an input.
  $effect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (usage.open || space.current !== "agents") return;
      if (e.key !== "n" || e.metaKey || e.ctrlKey || e.altKey) return;
      const active = document.activeElement as HTMLElement | null;
      if (
        active &&
        (active.tagName === "INPUT" ||
          active.tagName === "TEXTAREA" ||
          active.isContentEditable)
      ) {
        return;
      }
      if (store.selectedProjectId) {
        e.preventDefault();
        store.startDraft();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  /**
   * On a phone the agent, or a new one being drafted, takes the whole screen,
   * and the list comes back when it's left (`store.closeDetail`).
   */
  const detailOnPhone = $derived(
    viewport.phone && (!!store.selectedAgent || store.drafting),
  );

  /** Where a touch began, while it might yet be a swipe back to the list. */
  let swipeFrom: Point | null = null;

  function touchStart(e: TouchEvent) {
    const t = e.touches[0];
    swipeFrom = detailOnPhone && e.touches.length === 1 ? { x: t.clientX, y: t.clientY } : null;
  }

  function touchEnd(e: TouchEvent) {
    const t = e.changedTouches[0];
    if (swipeFrom && t && isBackSwipe(swipeFrom, { x: t.clientX, y: t.clientY })) {
      store.closeDetail();
    }
    swipeFrom = null;
  }

  const slide = { x: 40, duration: 200, easing: cubicOut };
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="agents" ontouchstart={touchStart} ontouchend={touchEnd}>
  {#if viewport.phone}
    <!-- One screen at a time, sliding in from the side they're going to. -->
    {#if detailOnPhone}
      <div class="screen" in:fly={slide}><AgentPane /></div>
    {:else}
      <div class="screen" in:fly={{ ...slide, x: -slide.x }}><ProjectSidebar phone /></div>
    {/if}
  {:else}
    <ProjectSidebar collapsed={panes.railed} />
    <PaneDivider
      label="Resize project list"
      min={MIN_PROJECTS_DRAG}
      minLast={MIN_DETAIL}
      onresize={(w) => panes.setProjects(w)}
      onreset={() => panes.setProjects(null)}
    />
    <AgentPane />
  {/if}
</div>

<style>
  .agents {
    flex: 1;
    min-width: 0;
    display: flex;
  }

  .screen {
    flex: 1;
    min-width: 0;
    display: flex;
  }
</style>
