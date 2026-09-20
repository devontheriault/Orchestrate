<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import ProjectSidebar from "$lib/ProjectSidebar.svelte";
  import AgentList from "$lib/AgentList.svelte";
  import AgentStream from "$lib/AgentStream.svelte";
  import SpawnModal from "$lib/SpawnModal.svelte";
  import UsageWindow from "$lib/UsageWindow.svelte";
  import PaneDivider from "$lib/PaneDivider.svelte";
  import { store } from "$lib/store.svelte";
  import { usage } from "$lib/usage.svelte";
  import { viewport } from "$lib/viewport.svelte";
  import {
    panes,
    MIN_AGENTS,
    MIN_DETAIL,
    MIN_PROJECTS,
    MIN_PROJECTS_DRAG,
  } from "$lib/panes.svelte";

  let showSpawn = $state(false);

  onMount(() => {
    viewport.start();
    store.start();
  });

  onDestroy(() => {
    store.stop();
    viewport.stop();
  });

  // Global "n" opens the Spawn modal, if a project is selected and the user
  // is not currently typing in an input.
  $effect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (showSpawn || usage.open) return;
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
        showSpawn = true;
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

  // Ctrl+Alt+R reloads the UI. Deliberately unconditional — it works with the
  // modal open and while typing, since a reload is the way out of a wedged
  // view. `code` rather than `key` so it survives non-QWERTY layouts.
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

  // Narrow windows can't hold the agent list and the detail pane side by side,
  // so they show one at a time: the list until an agent is picked, then the
  // agent, with a way back.
  const narrow = $derived(viewport.layout === "narrow");
  const showList = $derived(!narrow || !store.selectedAgentId);
  const showDetail = $derived(!narrow || !!store.selectedAgentId);

  // Dragged pane widths, held to what the current window can actually fit.
  const sized = $derived(panes.fit(viewport.width, showList && showDetail));

  // A hand-sized project pane decides for itself whether it's a rail; an
  // untouched one follows the window.
  const railed = $derived(
    sized.projects === null ? viewport.railed : sized.projects < MIN_PROJECTS,
  );

  const paneStyle = $derived(
    [
      sized.projects !== null ? `--pane-projects: ${sized.projects}px` : "",
      sized.agents !== null ? `--pane-agents: ${sized.agents}px` : "",
    ]
      .filter(Boolean)
      .join("; "),
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
  <ProjectSidebar collapsed={railed} />
  <PaneDivider
    label="Resize project list"
    min={MIN_PROJECTS_DRAG}
    minLast={showList && showDetail ? MIN_AGENTS + MIN_DETAIL : MIN_DETAIL}
    onresize={(w) => panes.setProjects(w)}
    onreset={() => panes.setProjects(null)}
  />
  {#if showList}
    <AgentList onSpawn={() => (showSpawn = true)} fill={narrow} />
  {/if}
  {#if showList && showDetail}
    <PaneDivider
      label="Resize agent list"
      min={MIN_AGENTS}
      minLast={MIN_DETAIL}
      onresize={(w) => panes.setAgents(w)}
      onreset={() => panes.setAgents(null)}
    />
  {/if}
  {#if showDetail}
    <AgentStream onBack={narrow ? () => store.selectAgent(null) : undefined} />
  {/if}
</main>

{#if showSpawn}
  <SpawnModal onClose={() => (showSpawn = false)} />
{/if}

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
    background: #fee2e2;
    color: #991b1b;
    padding: 0.55rem var(--pad-x);
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.85rem;
    border-bottom: 1px solid #fca5a5;
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
    font-size: 1.1rem;
    padding: 0 0.5rem;
  }

  .orphan-banner {
    background: rgba(245, 158, 11, 0.12);
    border-bottom: 1px solid rgba(245, 158, 11, 0.35);
    color: var(--fg);
    padding: 0.55rem var(--pad-x);
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem 1rem;
    font-size: 0.88rem;
  }

  .orphan-banner .actions {
    display: flex;
    gap: 0.4rem;
    flex: none;
    margin-left: auto;
  }

  .orphan-banner button {
    padding: 0.3rem 0.75rem;
    border-radius: 5px;
    border: 1px solid var(--border);
    background: var(--surface);
    color: var(--fg);
    cursor: pointer;
    font-size: 0.82rem;
    font-family: inherit;
  }

  .orphan-banner button:hover {
    border-color: #f59e0b;
  }

  .orphan-banner button.primary {
    background: #f59e0b;
    color: #1a1206;
    border-color: #f59e0b;
    font-weight: 500;
  }

  .orphan-banner button.primary:hover {
    filter: brightness(1.08);
  }

  @media (prefers-color-scheme: dark) {
    .error {
      background: #451a1a;
      color: #fca5a5;
      border-color: #7f1d1d;
    }
  }
</style>
