<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { tick } from "svelte";
  import { flip } from "svelte/animate";
  import { cubicOut } from "svelte/easing";
  import { api } from "$lib/api";
  import { mobile } from "$lib/layout/platform";
  import { hosts } from "$lib/state/hosts.svelte";
  import { orderProjects } from "$lib/state/projects";
  import { store } from "$lib/state/store.svelte";
  import AgentTree from "./AgentTree.svelte";
  import HostsDialog from "./HostsDialog.svelte";
  import { moveTo, slotFor } from "./reorder";
  import SettingsMenu from "./SettingsMenu.svelte";
  import TrustProject from "./TrustProject.svelte";

  let {
    collapsed = false,
    phone = false,
  }: {
    /** Rail mode: initials only, for windows too narrow to spare the width. */
    collapsed?: boolean;
    /**
     * The whole screen of a phone, taken in turns with the agent. Rows are
     * sized for a thumb, and they don't drag: on a touch screen a drag is a
     * scroll. Projects aren't removed from here either — a stray tap on the
     * phone is too easy, and the list belongs to the machines anyway.
     */
    phone?: boolean;
  } = $props();

  /** The rail has no room to nest agents, so they break out beside it. */
  type Flyout = { id: string; top: number; left: number; maxHeight: number };
  let flyout = $state<Flyout | null>(null);
  let asideEl: HTMLElement | undefined = $state();
  let flyoutEl: HTMLElement | undefined = $state();

  const FLYOUT_MAX = 380;

  /**
   * A folder the user picked, held until they say they trust it — and whether
   * it needs Git set up first, which the same confirmation agrees to.
   */
  let trusting = $state<{ path: string; needsSetup: boolean } | null>(null);

  async function pickAndAdd() {
    const picked = await open({ directory: true, multiple: false });
    if (typeof picked !== "string") return;
    trusting = { path: picked, needsSetup: await api.projectNeedsSetup(picked) };
  }

  /**
   * A phone has no folders of its own to add: its projects are the ones on
   * the machines it's added, so adding is adding a machine.
   */
  let addingHost = $state(false);
  const add = mobile ? () => (addingHost = true) : pickAndAdd;
  const addLabel = mobile ? "Add a machine" : "Add a project";

  async function addTrusted() {
    if (!trusting) return;
    const { path, needsSetup } = trusting;
    const name = path.split("/").filter(Boolean).pop() ?? path;
    // Held open until done: setting up a large folder can take a moment.
    await store.addProject(name, path, needsSetup);
    trusting = null;
  }

  /**
   * A project being dragged to a new place: the order it would drop into, and
   * how far the row sits from its slot, so it stays under the pointer.
   */
  let drag = $state<{ id: string; order: string[]; offset: number } | null>(null);
  let listEl: HTMLUListElement | undefined = $state();

  /** A drag just ended, so the click it ends in doesn't also open the row. */
  let dropped = false;

  /** How far the pointer moves before a press on a row becomes a drag. */
  const DRAG_THRESHOLD = 4;
  /** How near the list's edge the pointer scrolls it, and by how much a move. */
  const EDGE = 28;
  const EDGE_STEP = 10;

  const shown = $derived(drag ? orderProjects(store.projects, drag.order) : store.projects);

  function pressRow(e: PointerEvent, id: string) {
    dropped = false;
    if (e.button !== 0 || phone) return;
    const li = (e.currentTarget as HTMLElement).closest("li")!;
    const startY = e.clientY;
    const grab = e.clientY - li.getBoundingClientRect().top;
    let pointerY = startY;

    // Rows are placed by layout, not their on-screen boxes, so neighbours
    // mid-glide don't throw the sums off.
    const place = async () => {
      if (!drag || !listEl) return;
      const rows = [...listEl.querySelectorAll<HTMLElement>(":scope > li")];
      const self = rows.find((r) => r.dataset.project === id);
      if (!self) return;
      const at = pointerY - listEl.getBoundingClientRect().top + listEl.scrollTop - grab;
      const others = rows.filter((r) => r !== self).map((r) => r.offsetHeight);
      const top = Math.min(...rows.map((r) => r.offsetTop));
      const slot = slotFor(others, top, at);
      const order = moveTo(drag.order, id, slot);
      if (order.some((x, i) => x !== drag!.order[i])) {
        drag.order = order;
        await tick();
      }
      if (drag) drag.offset = at - self.offsetTop;
    };

    const onMove = (ev: PointerEvent) => {
      pointerY = ev.clientY;
      if (!drag) {
        if (Math.abs(pointerY - startY) < DRAG_THRESHOLD) return;
        drag = { id, order: store.projects.map((p) => p.id), offset: 0 };
        flyout = null;
      }
      if (listEl) {
        const box = listEl.getBoundingClientRect();
        if (pointerY < box.top + EDGE) listEl.scrollTop -= EDGE_STEP;
        else if (pointerY > box.bottom - EDGE) listEl.scrollTop += EDGE_STEP;
      }
      void place();
    };
    const end = () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("pointercancel", onCancel);
      window.removeEventListener("keydown", onKey, true);
    };
    const onUp = () => {
      end();
      if (!drag) return;
      dropped = true;
      // Settle into the slot rather than snapping to it.
      const row = listEl?.querySelector<HTMLElement>(`:scope > li[data-project="${CSS.escape(id)}"]`);
      row?.animate([{ transform: `translateY(${drag.offset}px)` }, { transform: "none" }], {
        duration: 150,
        easing: "cubic-bezier(0.2, 0, 0, 1)",
      });
      store.reorderProjects(drag.order);
      drag = null;
    };
    const onCancel = () => {
      end();
      if (drag) dropped = true;
      drag = null;
    };
    const onKey = (ev: KeyboardEvent) => {
      if (ev.key !== "Escape" || !drag) return;
      ev.stopPropagation();
      onCancel();
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    window.addEventListener("pointercancel", onCancel);
    window.addEventListener("keydown", onKey, true);
  }

  /** Alt+↑ / Alt+↓ on a row: the keyboard's way to drag it. */
  async function nudge(e: KeyboardEvent, id: string) {
    if (!e.altKey || (e.key !== "ArrowUp" && e.key !== "ArrowDown")) return;
    e.preventDefault();
    const row = e.currentTarget as HTMLElement;
    const ids = store.projects.map((p) => p.id);
    const to = ids.indexOf(id) + (e.key === "ArrowUp" ? -1 : 1);
    if (to < 0 || to >= ids.length) return;
    store.reorderProjects(moveTo(ids, id, to));
    await tick();
    row.focus();
  }

  /** Select the project, and show its agents wherever there's room for them. */
  function openProject(e: MouseEvent, id: string) {
    if (dropped) {
      dropped = false;
      return;
    }
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

<aside bind:this={asideEl} class:collapsed class:phone class:dragging={!!drag}>
  <header>
    {#if !collapsed}<span class="title">Projects</span>{/if}
    <button class="btn btn-icon add" onclick={add} title={addLabel} aria-label={addLabel}
      >+</button
    >
  </header>

  {#if store.projects.length === 0}
    {#if !store.loaded}
      <!-- The Host hasn't answered yet: saying "no projects" now would be
           the first thing on screen, and wrong. -->
    {:else if collapsed}
      <div class="empty-rail">
        <button onclick={add} title={addLabel} aria-label={addLabel}>+</button>
      </div>
    {:else if mobile && hosts.list.length === 0}
      <div class="empty">
        Add one of your machines to see its projects and agents.<br />
        <button class="btn btn-lg" onclick={add}>{addLabel}</button>
      </div>
    {:else if mobile && !hosts.list.some((h) => hosts.reachable(h.id))}
      <!-- They may well have projects: they just can't say so right now. -->
      <div class="empty">
        Your projects show here once one of your machines can be reached.<br />
        <button class="btn btn-lg" onclick={add}>{addLabel}</button>
      </div>
    {:else}
      <div class="empty">
        No projects yet.<br />
        <button class="btn btn-lg" onclick={add}>{addLabel}</button>
      </div>
    {/if}
  {:else}
    <ul bind:this={listEl} onscroll={() => (flyout = null)}>
      {#each shown as p (p.id)}
        {@const activity = store.activityFor(p.id)}
        {@const expanded = !collapsed && !!store.expandedProjects[p.id]}
        {@const lifted = drag?.id === p.id}
        <li
          data-project={p.id}
          class:selected={store.selectedProjectId === p.id}
          class:open={expanded}
          class:lifted
          style:transform={lifted ? `translateY(${drag!.offset}px)` : undefined}
          animate:flip={{ duration: lifted ? 0 : 180, easing: cubicOut }}
        >
          <div class="head">
            <button
              class="row"
              class:active={activity.running > 0}
              onpointerdown={(e) => pressRow(e, p.id)}
              onclick={(e) => openProject(e, p.id)}
              onkeydown={(e) => nudge(e, p.id)}
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
                    <svg class="badge-icon spinner" viewBox="0 0 10 10" aria-hidden="true">
                      <circle cx="5" cy="5" r="3.75" opacity="0.3" />
                      <path d="M5 1.25a3.75 3.75 0 0 1 3.75 3.75" />
                    </svg>
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
                    <svg class="badge-icon" viewBox="0 0 10 10" aria-hidden="true">
                      <path d="M2 5.25l2 2 4-4.5" />
                    </svg>
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
            {#if !collapsed && !phone}
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

{#if trusting}
  <TrustProject
    path={trusting.path}
    needsSetup={trusting.needsSetup}
    onconfirm={addTrusted}
    oncancel={() => (trusting = null)}
  />
{/if}

{#if addingHost}
  <HostsDialog onclose={() => (addingHost = false)} />
{/if}

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

  /* Everything in the sidebar reads one step up the type scale — the agent
     tree, settings menu and rail flyout included. `--control-font-size` is
     restated because it resolves at :root, not here. */
  aside,
  .flyout {
    --text-3xs: 0.7rem;
    --text-2xs: 0.74rem;
    --text-xs: 0.8rem;
    --text-sm: 0.84rem;
    --text-md: 0.89rem;
    --text-lg: 0.95rem;
    --text-xl: 1.05rem;
    --text-2xl: 1.15rem;
    --text-3xl: 1.25rem;
    --control-font-size: var(--text-md);
  }

  aside.collapsed {
    width: var(--rail);
    flex-basis: var(--rail);
  }

  aside.phone {
    width: auto;
    flex: 1 1 auto;
  }

  /* Rows a thumb can land on, and the "+" a full 44px target. */
  aside.phone .row {
    padding-block: 0.85rem;
  }

  aside.phone .add {
    width: 2.75rem;
    height: 2.75rem;
    margin-right: -0.6rem;
  }

  /* Above the home indicator. */
  aside.phone footer {
    padding-bottom: calc(var(--space-3) + var(--safe-bottom));
  }

  header {
    padding: var(--pad-y) var(--pad-x);
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    min-height: 2.8rem;
  }

  /* Left-aligned with the rail's rows and the settings button below. */
  aside.collapsed header {
    justify-content: flex-start;
    padding-inline: 0.4rem;
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
    /* Rows measure their place against the list, for dragging. */
    position: relative;
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
  }

  /* An open project keeps its own row tinted; the guide line under it does
     the work of showing which agents belong to it. */
  li.open {
    padding-bottom: 0.1rem;
  }

  /* Only a faint wash: the selected agent below carries the real highlight. */
  li.selected .head {
    background: color-mix(in srgb, var(--selected) 35%, transparent);
  }

  .head {
    display: flex;
    align-items: stretch;
    position: relative;
  }

  .head:hover,
  li.selected .head:hover {
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

  /* Status sits at the row's far edge, so badges line up down the list. */
  .name-row .badge {
    margin-left: auto;
  }

  /* Out past the room the row keeps for "×", to end where the agent timers
     under it do: the tree's inset plus an agent row's. */
  aside:not(.collapsed) .name-row .badge {
    margin-right: calc(var(--space-3) + 0.5rem - 1.7rem);
  }

  /* Which leaves it under "×", so it gives way while that shows. */
  aside:not(.collapsed) .head:hover .badge {
    visibility: hidden;
  }

  aside.collapsed .badge {
    padding: var(--space-1);
  }

  /* Says what the count is — still working, or done — where a dot only
     repeated the pill's colour. */
  .badge-icon {
    flex: none;
    width: 0.62rem;
    height: 0.62rem;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.6;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .spinner {
    animation: badge-spin 0.9s linear infinite;
  }

  @keyframes badge-spin {
    to {
      transform: rotate(360deg);
    }
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
    background: var(--float-bg, var(--panel-bg));
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

  /* The row being dragged is picked up off the list: raised like a popover,
     and opaque so the rows it passes don't show through. The border is a ring
     so it doesn't change the row's height mid-drag. */
  li.lifted {
    z-index: 1;
    background: var(--float-bg, var(--surface));
    -webkit-backdrop-filter: var(--float-filter, none);
    backdrop-filter: var(--float-filter, none);
    box-shadow:
      0 0 0 var(--border-width) var(--border),
      var(--shadow-popover);
    border-radius: var(--radius-md);
  }

  aside.dragging,
  aside.dragging * {
    cursor: grabbing !important;
    user-select: none;
  }

  /* Nothing under the pointer is a click target while it drags. */
  aside.dragging .head:hover {
    background: none;
  }

  aside:not(.collapsed).dragging .head:hover .badge {
    visibility: visible;
  }

  aside.dragging li.selected .head {
    background: color-mix(in srgb, var(--selected) 35%, transparent);
  }

  aside.dragging .head .remove {
    opacity: 0;
  }
</style>
