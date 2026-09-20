<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import ProjectSidebar from "$lib/ProjectSidebar.svelte";
  import AgentList from "$lib/AgentList.svelte";
  import AgentStream from "$lib/AgentStream.svelte";
  import SpawnModal from "$lib/SpawnModal.svelte";
  import { store } from "$lib/store.svelte";

  let showSpawn = $state(false);

  onMount(() => {
    store.start();
  });

  onDestroy(() => {
    store.stop();
  });

  // Global "n" opens the Spawn modal, if a project is selected and the user
  // is not currently typing in an input.
  $effect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (showSpawn) return;
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

  const showOrphanBanner = $derived(
    store.orphans.length > 0 && !store.orphanBannerDismissed,
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

<main>
  <ProjectSidebar />
  <AgentList onSpawn={() => (showSpawn = true)} />
  <AgentStream />
</main>

{#if showSpawn}
  <SpawnModal onClose={() => (showSpawn = false)} />
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
    padding: 0.55rem 0.9rem;
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-size: 0.85rem;
    border-bottom: 1px solid #fca5a5;
  }

  .error button {
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
    padding: 0.55rem 1rem;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    font-size: 0.88rem;
  }

  .orphan-banner .actions {
    display: flex;
    gap: 0.4rem;
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
