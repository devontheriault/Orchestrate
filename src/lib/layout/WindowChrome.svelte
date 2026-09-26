<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { dragRegion, ownsWindowFrame, watchMaximized } from "./platform";
  import { panes } from "./panes.svelte";
  import Logo from "./Logo.svelte";
  import { store } from "$lib/state/store.svelte";

  // Drives the maximize/restore glyph. Only tracked where this app draws the
  // button — macOS has its own and never asks.
  let maximized = $state(false);

  /** What the detail pane is showing, which is what the bar names. */
  const agent = $derived(store.selectedAgent);
  const drafting = $derived(store.drafting && !agent);
  const projectName = $derived(
    store.projects.find((p) => p.id === agent?.project_id)?.name ?? "",
  );

  $effect(() => {
    if (!ownsWindowFrame) return;
    return watchMaximized(getCurrentWindow(), (m) => (maximized = m));
  });
</script>

<!-- The whole bar is a drag handle, except under a tiling compositor (see
     `dragRegion`). The controls below opt out by not carrying the attribute, so
     a click on a button never starts a window drag. -->
<header class="chrome" data-tauri-drag-region={dragRegion}>
  <!-- The title sits in a segment as wide as the project pane and painted like
       it, so the bar reads as the top of the two panes below rather than as a
       band laid across them. -->
  <div
    class="lead"
    class:railed={panes.railed}
    style="width: {panes.cssWidth}"
    data-tauri-drag-region={dragRegion}
  >
    <Logo markOnly={panes.railed} />
  </div>

  <!-- The detail pane's heading, in the bar that continues it: the name Claude
       gave the work, with the prompt behind it a hover away. -->
  <div class="title" data-tauri-drag-region={dragRegion}>
    {#if agent}
      <span class="name" title={agent.task.prompt} data-tauri-drag-region={dragRegion}>
        {#if projectName}
          <span class="project">{projectName}</span>
          <span class="sep">/</span>
        {/if}
        {store.agentName(agent)}
      </span>
    {:else if drafting}
      <!-- Cancel sits up here rather than in a pane header of its own, which
           would have held nothing else. Its label is smaller than the title,
           so centring the two boxes leaves its text sitting high; the label
           shares the title's baseline instead. -->
      <div class="draft" data-tauri-drag-region={dragRegion}>
        <span class="name" data-tauri-drag-region={dragRegion}>New agent</span>
        <button class="cancel" onclick={() => store.cancelDraft()}>Cancel</button>
      </div>
    {:else}
      <span class="idle" data-tauri-drag-region={dragRegion}>No agent selected</span>
    {/if}
  </div>

  {#if ownsWindowFrame}
    <div class="controls">
      <button class="ctl" onclick={() => getCurrentWindow().minimize()} aria-label="Minimize">
        <svg viewBox="0 0 10 10" aria-hidden="true"><path d="M0 5h10" /></svg>
      </button>
      <button
        class="ctl"
        onclick={() => getCurrentWindow().toggleMaximize()}
        aria-label={maximized ? "Restore" : "Maximize"}
      >
        {#if maximized}
          <svg viewBox="0 0 10 10" aria-hidden="true"
            ><path d="M2.5 0.5h7v7h-7z" /><path d="M0.5 2.5h7v7h-7z" /></svg
          >
        {:else}
          <svg viewBox="0 0 10 10" aria-hidden="true"
            ><path d="M0.5 0.5h9v9h-9z" /></svg
          >
        {/if}
      </button>
      <button class="ctl close" onclick={() => getCurrentWindow().close()} aria-label="Close">
        <svg viewBox="0 0 10 10" aria-hidden="true"
          ><path d="M0.5 0.5l9 9M9.5 0.5l-9 9" /></svg
        >
      </button>
    </div>
  {/if}
</header>

<style>
  .chrome {
    flex: none;
    height: var(--titlebar-h);
    display: flex;
    align-items: stretch;
    justify-content: space-between;
    padding: 0;
    /* Continues the detail pane below it; the lead segment carries the project
       pane's colour. */
    background: var(--surface);
    user-select: none;
    -webkit-user-select: none;
    /* Over the usage overlay's backdrop (z-index 100). The OS bar used to sit
       outside the document and stayed reachable behind any modal; this one has
       to earn that back, or opening usage would strand the close button. */
    position: relative;
    z-index: var(--z-modal);
  }

  /* Leave the traffic lights their corner, and never get shorter than they
     are — at the small end of the root font scale the bar could clip them.
     Keyed off `<html data-frame>` rather than anything this component
     decides, because the page arrives rendered without knowing the platform
     (see `platform.ts`). */
  :global(html[data-frame="mac"]) .chrome {
    height: max(var(--titlebar-h), 28px);
  }

  .lead {
    flex: none;
    display: flex;
    align-items: center;
    padding: 0 var(--pad-x);
    background: var(--panel-bg);
    overflow: hidden;
    /* Sizes the logo: the name, not a caption, so it fills the bar the way a
       heading would. */
    font-size: 1.75rem;
  }

  /* Over the rail, the mark sits where the rail's own rows and "+" start. */
  .lead.railed {
    padding-inline: 0.4rem;
  }

  /* The traffic lights live in this corner, so the title starts after them. */
  :global(html[data-frame="mac"]) .lead {
    padding-left: 5rem;
  }


  .title {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    padding: 0 var(--pad-x);
    overflow: hidden;
  }

  .name {
    font-size: var(--text-xl);
    font-weight: var(--weight-semibold);
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* Muted colour alone separates the project from the agent name; both stay
     bold so the whole title reads as one heading. */
  .name .project,
  .name .sep {
    color: var(--fg-muted);
  }

  .name .sep {
    margin: 0 0.1rem;
  }

  .draft {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: baseline;
  }

  .cancel {
    flex: none;
    margin-left: auto;
    white-space: nowrap;
    border-radius: var(--radius-sm);
    border: 1px solid var(--border);
    background: var(--surface);
    padding: 0.2rem 0.75rem;
    font-size: var(--text-md);
    font-family: inherit;
    color: var(--fg);
    cursor: pointer;
    /* Out of the row's height, so the title alone sizes it and stays centred
       in the bar exactly as it does over an agent; the button just hangs off
       the shared baseline. */
    margin-block: -1rem;
  }

  .cancel:hover { border-color: var(--accent); color: var(--accent); }

  .idle {
    font-size: var(--text-xl);
    color: var(--fg-muted);
    font-style: italic;
  }

  .controls {
    display: flex;
    align-items: stretch;
    flex: none;
    margin-left: auto;
  }

  /* The page arrives rendered for the app's own frame, controls and all, and
     only drops them once the app takes over; until then they're hidden
     wherever the OS or the compositor owns the frame. */
  :global(html:not([data-frame="app"])) .controls {
    display: none;
  }

  .ctl {
    width: 2.6rem;
    height: auto;
    display: grid;
    place-items: center;
    background: transparent;
    border: none;
    color: var(--fg-muted);
    cursor: pointer;
    padding: 0;
  }

  .ctl svg {
    width: 0.62rem;
    height: 0.62rem;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.1;
  }

  .ctl:hover {
    background: var(--hover);
    color: var(--fg);
  }

  .ctl.close:hover {
    background: var(--window-close-bg);
    color: var(--window-close-fg);
  }
</style>
