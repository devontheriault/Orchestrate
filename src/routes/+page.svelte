<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import ProjectSidebar from "$lib/ProjectSidebar.svelte";
  import AgentStream from "$lib/AgentStream.svelte";
  import UsageWindow from "$lib/UsageWindow.svelte";
  import PaneDivider from "$lib/PaneDivider.svelte";
  import { store } from "$lib/store.svelte";
  import { usage } from "$lib/usage.svelte";
  import { viewport } from "$lib/viewport.svelte";
  import { panes, MIN_DETAIL, MIN_PROJECTS_DRAG } from "$lib/panes.svelte";

  onMount(() => {
    viewport.start();
    store.start();
  });

  onDestroy(() => {
    store.stop();
    viewport.stop();
  });

  // Global "n" opens the blank page for a new agent, if a project is selected
  // and the user is not currently typing in an input.
  $effect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (usage.open) return;
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

  // Ctrl+Shift+U toggles the token-usage window. Unconditional like the reload
  // below — a glance at what the agents are spending shouldn't depend on where
  // the focus happens to be. `code` so it survives non-QWERTY layouts.
  $effect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!e.ctrlKey || !e.shiftKey || e.metaKey || e.altKey) return;
      if (e.code !== "KeyU" && e.key.toLowerCase() !== "u") return;
      e.preventDefault();
      usage.toggle();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  // Ctrl+Alt+R reloads the UI. Deliberately unconditional — it works while
  // typing, since a reload is the way out of a wedged view. `code` rather than
  // `key` so it survives non-QWERTY layouts.
  $effect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!e.ctrlKey || !e.altKey || e.metaKey || e.shiftKey) return;
      if (e.code !== "KeyR" && e.key.toLowerCase() !== "r") return;
      e.preventDefault();
      store.reload();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const showOrphanBanner = $derived(
    store.orphans.length > 0 && !store.orphanBannerDismissed,
  );

  // A hand-sized pane keeps its width once it collapses, so the seam carries on
  // tracking the pointer through the rail instead of snapping to the stylesheet
  // rail and sitting there for the rest of the drag. An untouched pane still
  // gets the rail the stylesheet picked.
  const paneStyle = $derived(
    panes.width !== null
      ? `--pane-projects: ${panes.width}px; --rail: ${panes.width}px`
      : "",
  );
</script>

{#if store.error}
  <div class="error" role="alert">
    {store.error}
    <button onclick={() => (store.error = null)} aria-label="Dismiss error">×</button>
  </div>
{/if}

{#if showOrphanBanner}
  <div class="orphan-banner" role="status">
    <div>
      <strong>{store.orphans.length}</strong>
      agent{store.orphans.length === 1 ? "" : "s"}
      {store.orphans.length === 1 ? "was" : "were"} running when the app was last closed.
    </div>
    <div class="actions">
      <button class="primary" onclick={() => store.jumpToFirstOrphan()}>
        Show
      </button>
      <button onclick={() => (store.orphanBannerDismissed = true)}>Dismiss</button>
    </div>
  </div>
{/if}

<main style={paneStyle}>
  <ProjectSidebar collapsed={panes.railed} />
  <PaneDivider
    label="Resize project list"
    min={MIN_PROJECTS_DRAG}
    minLast={MIN_DETAIL}
    onresize={(w) => panes.setProjects(w)}
    onreset={() => panes.setProjects(null)}
  />
  <AgentStream />
</main>

{#if usage.open}
  <UsageWindow />
{/if}

<style>
  main {
    display: flex;
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }

  .error {
    background: var(--danger-bg);
    color: var(--danger-fg);
    padding: 0.55rem var(--pad-x);
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: var(--space-4);
    font-size: var(--text-md);
    border-bottom: 1px solid var(--danger-border);
    /* A long backend error wraps rather than pushing the dismiss off-screen. */
    overflow-wrap: anywhere;
    max-height: 30vh;
    overflow-y: auto;
  }

  .error button {
    flex: none;
    background: transparent;
    border: none;
    color: inherit;
    cursor: pointer;
    font-size: var(--text-3xl);
    padding: 0 0.5rem;
  }

  .orphan-banner {
    background: var(--warning-soft-bg);
    border-bottom: 1px solid var(--warning-soft-border);
    color: var(--fg);
    padding: 0.55rem var(--pad-x);
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem 1rem;
    font-size: var(--text-lg);
  }

  .orphan-banner .actions {
    display: flex;
    gap: var(--space-3);
    flex: none;
    margin-left: auto;
  }

  .orphan-banner button {
    padding: 0.3rem 0.75rem;
    border-radius: var(--radius-sm);
    border: 1px solid var(--border);
    background: var(--surface);
    color: var(--fg);
    cursor: pointer;
    font-size: var(--text-md);
    font-family: inherit;
  }

  .orphan-banner button:hover {
    border-color: var(--warning);
  }

  .orphan-banner button.primary {
    background: var(--warning);
    color: var(--on-warning);
    border-color: var(--warning);
    font-weight: var(--weight-medium);
  }

  .orphan-banner button.primary:hover {
    filter: brightness(1.08);
  }
</style>
