<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { store } from "./store.svelte";

  async function pickAndAdd() {
    const picked = await open({ directory: true, multiple: false });
    if (typeof picked !== "string") return;
    const name = picked.split("/").filter(Boolean).pop() ?? picked;
    await store.addProject(name, picked);
  }
</script>

<aside>
  <header>
    <span class="title">Projects</span>
    <button class="add" onclick={pickAndAdd} title="Add project">+</button>
  </header>

  {#if store.projects.length === 0}
    <div class="empty">
      No projects yet.<br />
      <button onclick={pickAndAdd}>Add a project</button>
    </div>
  {:else}
    <ul>
      {#each store.projects as p (p.id)}
        <li class:selected={store.selectedProjectId === p.id}>
          <button
            class="row"
            onclick={() => store.selectProject(p.id)}
            aria-current={store.selectedProjectId === p.id ? "true" : undefined}
          >
            <span class="name">{p.name}</span>
            <span class="path">{p.path}</span>
          </button>
          <button
            class="remove"
            onclick={() => store.removeProject(p.id)}
            title="Remove from list"
            aria-label={`Remove ${p.name}`}>×</button
          >
        </li>
      {/each}
    </ul>
  {/if}
</aside>

<style>
  aside {
    width: 240px;
    flex: 0 0 240px;
    border-right: 1px solid var(--border);
    background: var(--panel-bg);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  header {
    padding: 0.75rem 0.9rem;
    display: flex;
    align-items: center;
    justify-content: space-between;
    border-bottom: 1px solid var(--border);
  }

  .title {
    font-size: 0.78rem;
    font-weight: 600;
    color: var(--fg-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .add {
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 4px;
    width: 22px;
    height: 22px;
    padding: 0;
    color: var(--fg);
    cursor: pointer;
    font-size: 1rem;
    line-height: 1;
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }

  .add:hover {
    border-color: var(--accent);
    color: var(--accent);
  }

  .empty {
    padding: 1.5rem 1rem;
    text-align: center;
    color: var(--fg-muted);
    font-size: 0.88rem;
  }

  .empty button {
    margin-top: 0.75rem;
    padding: 0.4rem 0.8rem;
    border-radius: 5px;
    border: 1px solid var(--border);
    background: var(--surface);
    color: var(--fg);
    cursor: pointer;
    font-family: inherit;
    font-size: 0.85rem;
  }

  .empty button:hover {
    border-color: var(--accent);
  }

  ul {
    list-style: none;
    padding: 0.25rem 0;
    margin: 0;
    overflow-y: auto;
    flex: 1;
  }

  li {
    display: flex;
    align-items: stretch;
    position: relative;
    border-left: 2px solid transparent;
  }

  li:hover {
    background: var(--hover);
  }

  li.selected {
    background: var(--selected);
    border-left-color: var(--accent);
  }

  .row {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    padding: 0.55rem 0.9rem;
    background: transparent;
    border: none;
    text-align: left;
    cursor: pointer;
    color: var(--fg);
    min-width: 0;
  }

  .name {
    font-size: 0.9rem;
    font-weight: 500;
  }

  .path {
    font-size: 0.72rem;
    color: var(--fg-muted);
    font-family: ui-monospace, monospace;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .remove {
    position: absolute;
    right: 0.5rem;
    top: 50%;
    transform: translateY(-50%);
    background: transparent;
    border: none;
    color: var(--fg-muted);
    cursor: pointer;
    font-size: 1.1rem;
    line-height: 1;
    padding: 0.15rem 0.35rem;
    border-radius: 3px;
    opacity: 0;
  }

  li:hover .remove {
    opacity: 1;
  }

  .remove:hover {
    background: var(--hover);
    color: var(--fg);
  }
</style>
