<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { store } from "./store.svelte";
  import { usage } from "./usage.svelte";
  import AgentTree from "./AgentTree.svelte";

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

<aside bind:this={asideEl} class:collapsed>
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
    <button
      class="usage"
      onclick={() => usage.show()}
      title="Token usage (Ctrl+Shift+U)"
      aria-label="Token usage"
    >
      <span class="gauge" aria-hidden="true">
        <svg viewBox="0 0 10 8" width="10" height="8">
          <rect x="0.5" y="5" width="2" height="3" rx="0.5" fill="currentColor" />
          <rect x="4" y="2.5" width="2" height="5.5" rx="0.5" fill="currentColor" />
          <rect x="7.5" y="0.5" width="2" height="7.5" rx="0.5" fill="currentColor" />
        </svg>
      </span>
      {#if !collapsed}<span class="label">Usage</span><kbd>⌃⇧U</kbd>{/if}
    </button>
  </footer>
</aside>

{#if flyout && flyoutProject}
  <div
    bind:this={flyoutEl}
    class="flyout"
    role="dialog"
    aria-label={`Agents in ${flyoutProject.name}`}
    style={`top: ${flyout.top}px; left: ${flyout.left}px; max-height: ${flyout.maxHeight}px`}
  >
    <div class="flyout-head">
      <span class="flyout-name" title={flyoutProject.path}>{flyoutProject.name}</span>
      <button onclick={() => (flyout = null)} aria-label="Close agent list">×</button>
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
    display: flex;
    align-items: center;
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
    gap: 0.15rem;
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
    gap: 0.4rem;
    min-width: 0;
  }

  aside.collapsed .name-row {
    gap: 0.2rem;
  }

  .chevron {
    flex: none;
    width: 0.6rem;
    font-size: 0.95rem;
    line-height: 1;
    color: var(--fg-muted);
    transition: transform 0.12s ease;
    transform-origin: 40% 50%;
  }

  .chevron.open {
    transform: rotate(90deg);
    color: var(--accent);
  }

  @media (prefers-reduced-motion: reduce) {
    .chevron {
      transition: none;
    }
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

  .head:hover .remove {
    opacity: 1;
  }

  .remove:hover {
    background: var(--hover);
    color: var(--fg);
  }

  /* Fixed, so the rail's own scrolling and overflow can't clip it. */
  .flyout {
    position: fixed;
    z-index: 40;
    width: 17rem;
    /* Anchored to the rail's right edge, so this is all the room there is. */
    max-width: calc(100vw - var(--rail) - 1rem);
    display: flex;
    flex-direction: column;
    background: var(--panel-bg);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.18);
    overflow: hidden;
  }

  .flyout-head {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.4rem 0.4rem 0.4rem 0.7rem;
    border-bottom: 1px solid var(--border);
  }

  .flyout-name {
    flex: 1;
    min-width: 0;
    font-size: 0.82rem;
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .flyout-head button {
    flex: none;
    background: transparent;
    border: none;
    color: var(--fg-muted);
    cursor: pointer;
    font-size: 1.1rem;
    line-height: 1;
    padding: 0.1rem 0.3rem;
    border-radius: 3px;
  }

  .flyout-head button:hover {
    background: var(--hover);
    color: var(--fg);
  }

  .flyout-body {
    overflow-y: auto;
    min-height: 0;
  }
</style>
