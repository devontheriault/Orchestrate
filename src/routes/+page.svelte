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
</script>

{#if store.error}
  <div class="error" role="alert">
    {store.error}
    <button onclick={() => (store.error = null)}>×</button>
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

  @media (prefers-color-scheme: dark) {
    .error {
      background: #451a1a;
      color: #fca5a5;
      border-color: #7f1d1d;
    }
  }
</style>
