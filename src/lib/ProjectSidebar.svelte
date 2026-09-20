<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { store } from "./store.svelte";
  import { usage } from "./usage.svelte";

  /** Rail mode: initials only, for windows too narrow to spare the width. */
  let { collapsed = false }: { collapsed?: boolean } = $props();

  async function pickAndAdd() {
    const picked = await open({ directory: true, multiple: false });
    if (typeof picked !== "string") return;
    const name = picked.split("/").filter(Boolean).pop() ?? picked;
    await store.addProject(name, picked);
  }

  /** The line under a project name: what's running, or the path when idle. */
  function subtitle(projectId: string, path: string): string {
    const { running, latestRunning } = store.activityFor(projectId);
    if (running === 0) return path;
    if (running > 1) return `${running} agents running`;
    const first = latestRunning?.task.prompt.split("\n")[0]?.trim();
    return first ? first : "1 agent running";
  }

  /** Stand-in for the name in rail mode. */
  function initials(name: string): string {
    const parts = name.split(/[\s._/-]+/).filter(Boolean);
    const from =
      parts.length > 1 ? parts[0][0] + parts[1][0] : name.replace(/\W/g, "").slice(0, 2);
    return (from || name.slice(0, 2)).toUpperCase();
  }
</script>

<aside class:collapsed>
  <header>
    {#if !collapsed}<span class="title">Projects</span>{/if}
    <button class="add" onclick={pickAndAdd} title="Add project" aria-label="Add project"
      >+</button
    >
  </header>

  {#if store.projects.length === 0}
    {#if collapsed}
      <div class="empty-rail">
        <button onclick={pickAndAdd} title="Add a project" aria-label="Add a project">+</button>
      </div>
    {:else}
      <div class="empty">
        No projects yet.<br />
        <button onclick={pickAndAdd}>Add a project</button>
      </div>
    {/if}
  {:else}
    <ul>
      {#each store.projects as p (p.id)}
        {@const activity = store.activityFor(p.id)}
        <li class:selected={store.selectedProjectId === p.id}>
          <button
            class="row"
            class:active={activity.running > 0}
            onclick={() => store.selectProject(p.id)}
            aria-current={store.selectedProjectId === p.id ? "true" : undefined}
            title={collapsed ? `${p.name} — ${p.path}` : p.path}
          >
            <span class="name-row">
              <span class="name">{collapsed ? initials(p.name) : p.name}</span>
              {#if activity.running > 0}
                <span
                  class="badge running"
                  title={`${activity.running} agent${activity.running === 1 ? "" : "s"} running`}
                >
                  <span class="dot"></span>
                  {#if !collapsed}{activity.running}{/if}
                </span>
              {:else if activity.attention > 0}
                <span
                  class="badge attention"
                  title={`${activity.attention} agent${activity.attention === 1 ? "" : "s"} need attention`}
                >
                  {activity.attention}
                </span>
              {/if}
            </span>
            {#if !collapsed}
              <span class="sub" class:path={activity.running === 0}>
                {subtitle(p.id, p.path)}
              </span>
            {/if}
          </button>
          {#if !collapsed}
            <button
              class="remove"
              onclick={() => store.removeProject(p.id)}
              title="Remove from list"
              aria-label={`Remove ${p.name}`}>×</button
            >
          {/if}
        </li>
      {/each}
    </ul>
  {/if}

  <footer>
    <button
      class="usage"
      onclick={() => usage.show()}
      title="Token usage (Ctrl+Shift+U)"
      aria-label="Token usage"
    >
      <span class="gauge">◔</span>
      {#if !collapsed}<span class="label">Usage</span><kbd>⌃⇧U</kbd>{/if}
    </button>
  </footer>
</aside>

<style>
  aside {
    width: var(--pane-projects);
    flex: 0 0 var(--pane-projects);
    /* The seam to the right is the draggable PaneDivider, not a border. */
    background: var(--panel-bg);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  aside.collapsed {
    width: var(--rail);
    flex-basis: var(--rail);
  }

  header {
    padding: var(--pad-y) var(--pad-x);
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.4rem;
    border-bottom: 1px solid var(--border);
    min-height: 2.8rem;
  }

  aside.collapsed header {
    justify-content: center;
    padding-inline: 0.25rem;
  }

  .title {
    font-size: 0.78rem;
    font-weight: 600;
    color: var(--fg-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .add {
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 4px;
    width: 1.6rem;
    height: 1.6rem;
    flex: none;
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

  .empty-rail {
    display: flex;
    justify-content: center;
    padding: 0.75rem 0;
  }

  .empty-rail button {
    width: 2rem;
    height: 2rem;
    border-radius: 6px;
    border: 1px dashed var(--border);
    background: transparent;
    color: var(--fg-muted);
    cursor: pointer;
    font-size: 1rem;
    line-height: 1;
  }

  .empty-rail button:hover {
    border-color: var(--accent);
    color: var(--accent);
  }

  ul {
    list-style: none;
    padding: 0.25rem 0;
    margin: 0;
    overflow-y: auto;
    overflow-x: hidden;
    flex: 1;
    min-height: 0;
  }

  /* Always reachable: the list above it scrolls, this doesn't. */
  footer {
    flex: none;
    margin-top: auto;
    border-top: 1px solid var(--border);
    padding: 0.35rem;
  }

  .usage {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 0.45rem;
    background: transparent;
    border: none;
    border-radius: 5px;
    padding: 0.35rem 0.45rem;
    color: var(--fg-muted);
    font-size: 0.8rem;
    cursor: pointer;
  }

  .usage:hover {
    background: var(--hover);
    color: var(--fg);
  }

  aside.collapsed .usage {
    justify-content: center;
    padding: 0.35rem 0;
  }

  .usage .gauge {
    font-size: 0.95rem;
    line-height: 1;
  }

  .usage .label {
    flex: 1;
    text-align: left;
  }

  .usage kbd {
    font-family: ui-monospace, monospace;
    font-size: 0.68rem;
    color: var(--fg-muted);
    background: var(--code-bg);
    border-radius: 3px;
    padding: 0.05em 0.3em;
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
    padding: 0.55rem 1.7rem 0.55rem var(--pad-x);
    background: transparent;
    border: none;
    text-align: left;
    cursor: pointer;
    color: var(--fg);
    min-width: 0;
  }

  aside.collapsed .row {
    padding: 0.5rem 0.25rem;
    align-items: center;
  }

  .name-row {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    min-width: 0;
  }

  aside.collapsed .name-row {
    gap: 0.2rem;
  }

  .name {
    font-size: 0.9rem;
    font-weight: 500;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  aside.collapsed .name {
    font-family: ui-monospace, monospace;
    font-size: 0.82rem;
    letter-spacing: 0.02em;
  }

  .row.active .name {
    font-weight: 600;
  }

  .badge {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
    gap: 0.28rem;
    font-size: 0.68rem;
    font-weight: 600;
    line-height: 1;
    padding: 0.16rem 0.38rem;
    border-radius: 999px;
    font-variant-numeric: tabular-nums;
  }

  aside.collapsed .badge {
    padding: 0.16rem;
  }

  .badge.running {
    color: var(--running);
    background: var(--running-bg);
  }

  .badge.attention {
    color: var(--attention);
    background: var(--attention-bg);
  }

  .badge .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--running);
    animation: pulse 2s infinite;
  }

  @keyframes pulse {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.35;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .badge .dot {
      animation: none;
    }
  }

  .sub {
    font-size: 0.72rem;
    color: var(--fg-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .row.active .sub {
    color: var(--running);
  }

  .sub.path {
    font-family: ui-monospace, monospace;
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
