<script lang="ts">
  /**
   * The Agents Space: the project list and the agent opened from it, side by
   * side, or one screen at a time on a phone.
   */
  import PhoneStack from "$lib/layout/PhoneStack.svelte";
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
  const detailOpen = $derived(!!store.selectedAgent || store.drafting);
</script>

<div class="agents">
  {#if viewport.phone}
    <PhoneStack space="agents" open={detailOpen} onclose={() => store.closeDetail()}>
      {#snippet list()}<ProjectSidebar phone />{/snippet}
      {#snippet detail()}<AgentPane />{/snippet}
    </PhoneStack>
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
</style>
