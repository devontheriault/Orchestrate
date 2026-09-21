<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { store } from "./store.svelte";
  import AgentTree from "./AgentTree.svelte";
  import SettingsMenu from "./SettingsMenu.svelte";

  /** Rail mode: initials only, for windows too narrow to spare the width. */
  let { collapsed = false }: { collapsed?: boolean } = $props();

  /** The rail has no room to nest agents, so they break out beside it. */
  type Flyout = { id: string; top: number; left: number; maxHeight: number };
  let flyout = $state<Flyout | null>(null);
  let asideEl: HTMLElement | undefined = $state();
  let flyoutEl: HTMLElement | undefined = $state();

  const FLYOUT_MAX = 380;

  async function pickAndAdd() {
    const picked = await open({ directory: true, multiple: false });
    if (typeof picked !== "string") return;
    const name = picked.split("/").filter(Boolean).pop() ?? picked;
    await store.addProject(name, picked);
  }

  /** Select the project, and show its agents wherever there's room for them. */
  function openProject(e: MouseEvent, id: string) {
    if (!collapsed) {
      store.toggleProject(id);
      return;
    }
    const showing = flyout?.id === id;
    store.selectProject(id);
    if (showing) {
      flyout = null;
      return;
    }
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const maxHeight = Math.min(FLYOUT_MAX, window.innerHeight - 16);
    flyout = {
      id,
      left: rect.right + 6,
      top: Math.max(8, Math.min(rect.top, window.innerHeight - maxHeight - 8)),
      maxHeight,
    };
  }

  const flyoutProject = $derived(
    flyout ? store.projects.find((p) => p.id === flyout!.id) ?? null : null,
  );

  // The flyout is pinned to a row's position, so anything that moves that row —
  // or takes the rail away — dismisses it rather than leaving it stranded.
  $effect(() => {
    if (!flyout) return;
    const close = () => (flyout = null);
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && close();
    const onDown = (e: PointerEvent) => {
      const t = e.target as Node;
      if (flyoutEl?.contains(t) || asideEl?.contains(t)) return;
      close();
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("pointerdown", onDown, true);
    window.addEventListener("resize", close);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("pointerdown", onDown, true);
      window.removeEventListener("resize", close);
    };
  });

  $effect(() => {
    if (!collapsed) flyout = null;
  });

  /** The line under a project name: what's running, or the path when idle. */
  function subtitle(projectId: string, path: string): string {
    const { running, latestRunning } = store.activityFor(projectId);
    if (running === 0) return path;
    if (running > 1) return `${running} agents running`;
    const name = latestRunning ? store.agentName(latestRunning).trim() : "";
    return name ? name : "1 agent running";
  }

  /** Stand-in for the name in rail mode. */
  function initials(name: string): string {
    const parts = name.split(/[\s._/-]+/).filter(Boolean);
    const from =
      parts.length > 1 ? parts[0][0] + parts[1][0] : name.replace(/\W/g, "").slice(0, 2);
    return (from || name.slice(0, 2)).toUpperCase();
  }
</script>

<aside bind:this={asideEl} class:collapsed>
  <header>
    {#if !collapsed}<span class="title">Projects</span>{/if}
    <button class="btn btn-icon add" onclick={pickAndAdd} title="Add project" aria-label="Add project"
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
        <button class="btn btn-lg" onclick={pickAndAdd}>Add a project</button>
      </div>
    {/if}
  {:else}
    <ul onscroll={() => (flyout = null)}>
      {#each store.projects as p (p.id)}
        {@const activity = store.activityFor(p.id)}
        {@const expanded = !collapsed && !!store.expandedProjects[p.id]}
        <li class:selected={store.selectedProjectId === p.id} class:open={expanded}>
          <div class="head">
            <button
              class="row"
              class:active={activity.running > 0}
              onclick={(e) => openProject(e, p.id)}
              aria-current={store.selectedProjectId === p.id ? "true" : undefined}
              aria-expanded={collapsed ? undefined : expanded}
              title={collapsed ? `${p.name} — ${p.path}` : p.path}
            >
              <span class="name-row">
                {#if !collapsed}
                  <span class="chevron" class:open={expanded} aria-hidden="true">›</span>
                {/if}
                <span class="name">{collapsed ? initials(p.name) : p.name}</span>
                {#if activity.running > 0}
                  <span
                    class="badge badge-running"
                    title={`${activity.running} agent${activity.running === 1 ? "" : "s"} running`}
                  >
                    <span class="badge-dot"></span>
                    {#if !collapsed}{activity.running}{/if}
                  </span>
                {:else if activity.attention > 0}
                  <span
                    class="badge badge-attention"
                    title={`${activity.attention} agent${activity.attention === 1 ? "" : "s"} need attention`}
                  >
                    {activity.attention}
                  </span>
                {:else if activity.completed > 0}
                  <span
                    class="badge badge-completed"
                    title={`${activity.completed} agent${activity.completed === 1 ? "" : "s"} completed`}
                  >
                    <span class="badge-dot"></span>
                    {#if !collapsed}{activity.completed}{/if}
                  </span>
                {/if}
              </span>
              {#if !collapsed && !expanded}
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
          </div>
          {#if expanded}
            <AgentTree projectId={p.id} />
          {/if}
        </li>
      {/each}
    </ul>
  {/if}

  <footer>
    <SettingsMenu {collapsed} />
  </footer>
</aside>

{#if flyout && flyoutProject}
  <div
    bind:this={flyoutEl}
    class="popover flyout"
    role="dialog"
    aria-label={`Agents in ${flyoutProject.name}`}
    style={`top: ${flyout.top}px; left: ${flyout.left}px; max-height: ${flyout.maxHeight}px`}
  >
    <div class="flyout-head">
      <span class="flyout-name" title={flyoutProject.path}>{flyoutProject.name}</span>
      <button class="btn btn-ghost btn-sm" onclick={() => (flyout = null)} aria-label="Close agent list"
        >×</button
      >
    </div>
    <div class="flyout-body">
      <AgentTree projectId={flyoutProject.id} flat onpick={() => (flyout = null)} />
    </div>
  </div>
{/if}

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
    gap: var(--space-3);
    min-height: 2.8rem;
  }

  aside.collapsed header {
    justify-content: center;
    padding-inline: 0.25rem;
  }

  .title {
    font-size: var(--text-sm);
    font-weight: var(--weight-semibold);
    color: var(--fg-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* A tighter square than `.btn-icon`, to sit in the header's row. */
  .add {
    width: 1.6rem;
    height: 1.6rem;
    flex: none;
    background: transparent;
    font-size: var(--text-2xl);
    line-height: var(--leading-none);
  }

  .empty {
    padding: 1.5rem 1rem;
    text-align: center;
    color: var(--fg-muted);
    font-size: var(--text-lg);
  }

  .empty :global(button) {
    margin-top: var(--space-5);
  }

  .empty-rail {
    display: flex;
    justify-content: center;
    padding: 0.75rem 0;
  }

  .empty-rail button {
    width: 2rem;
    height: 2rem;
    border-radius: var(--radius-md);
    border: 1px dashed var(--border);
    background: transparent;
    color: var(--fg-muted);
    cursor: pointer;
    font-size: var(--text-2xl);
    line-height: var(--leading-none);
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
    padding: var(--space-3);
  }

  li {
    position: relative;
    border-left: 2px solid transparent;
  }

  /* An open project keeps its own row tinted; the guide line under it does
     the work of showing which agents belong to it. */
  li.open {
    padding-bottom: 0.1rem;
  }

  li.selected {
    border-left-color: var(--accent);
  }

  li.selected .head {
    background: var(--selected);
  }

  .head {
    display: flex;
    align-items: stretch;
    position: relative;
  }

  .head:hover {
    background: var(--hover);
  }

  .row {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    padding: 0.55rem 1.7rem 0.55rem var(--pad-x);
    background: transparent;
    border: none;
    text-align: left;
    cursor: pointer;
    color: var(--fg);
    min-width: 0;
  }

  /* The rail still reads as a list, so its names start at the left edge like
     the tree's do — only the padding tightens. */
  aside.collapsed .row {
    padding: 0.5rem 0.4rem;
    align-items: flex-start;
  }

  .name-row {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-width: 0;
  }

  aside.collapsed .name-row {
    gap: var(--space-2);
  }

  .chevron {
    flex: none;
    width: 0.6rem;
    font-size: var(--text-xl);
    line-height: var(--leading-none);
    color: var(--fg-muted);
    transition: transform var(--transition-fast);
    transform-origin: 40% 50%;
  }

  .chevron.open {
    transform: rotate(90deg);
    color: var(--accent);
  }

  .name {
    font-size: var(--text-lg);
    font-weight: var(--weight-medium);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  aside.collapsed .name {
    font-family: var(--font-mono);
    font-size: var(--text-md);
    letter-spacing: 0.02em;
  }

  .row.active .name {
    font-weight: var(--weight-semibold);
  }

  aside.collapsed .badge {
    padding: var(--space-1);
  }

  .sub {
    font-size: var(--text-xs);
    color: var(--fg-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .row.active .sub {
    color: var(--running);
  }

  .sub.path {
    font-family: var(--font-mono);
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
    font-size: var(--text-3xl);
    line-height: var(--leading-none);
    padding: 0.15rem 0.35rem;
    border-radius: var(--radius-xs);
    opacity: 0;
  }

  .head:hover .remove {
    opacity: 1;
  }

  .remove:hover {
    background: var(--hover);
    color: var(--fg);
  }

  /* Fixed, so the rail's own scrolling and overflow can't clip it. */
  /* A `.popover`, on the rail's panel surface rather than the page's: it is
     the collapsed sidebar's own row, opened sideways. */
  .flyout {
    z-index: var(--z-titlebar);
    width: 17rem;
    /* Anchored to the rail's right edge, so this is all the room there is. */
    max-width: calc(100vw - var(--rail) - 1rem);
    display: flex;
    flex-direction: column;
    background: var(--panel-bg);
    overflow: hidden;
  }

  .flyout-head {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: 0.4rem 0.4rem 0.4rem 0.7rem;
    border-bottom: 1px solid var(--border);
  }

  .flyout-name {
    flex: 1;
    min-width: 0;
    font-size: var(--text-md);
    font-weight: var(--weight-semibold);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .flyout-head :global(button) {
    flex: none;
    font-size: var(--text-3xl);
    line-height: var(--leading-none);
  }

  .flyout-body {
    overflow-y: auto;
    min-height: 0;
  }
</style>
