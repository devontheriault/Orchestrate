<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { isMac, ownsWindowFrame } from "./platform";
  import { panes } from "./panes.svelte";
  import { store } from "$lib/state/store.svelte";

  const win = getCurrentWindow();

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
    let live = true;
    let unlisten: (() => void) | undefined;
    const sync = () => {
      win.isMaximized().then((m) => {
        if (live) maximized = m;
      });
    };
    sync();
    win.onResized(sync).then((f) => {
      if (live) unlisten = f;
      else f();
    });
    return () => {
      live = false;
      unlisten?.();
    };
  });
</script>

<!-- The whole bar is a drag handle; the controls below opt out by not carrying
     the attribute, so a click on a button never starts a window drag. -->
<header class="chrome" class:mac={isMac} data-tauri-drag-region>
  <!-- The title sits in a segment as wide as the project pane and painted like
       it, so the bar reads as the top of the two panes below rather than as a
       band laid across them. -->
  <div class="lead" style="width: {panes.cssWidth}" data-tauri-drag-region>
    <span class="wordmark" data-tauri-drag-region
      ><b data-tauri-drag-region>DEV</b> Code</span
    >
  </div>

  <!-- The detail pane's heading, in the bar that continues it: the name Claude
       gave the work, with the prompt behind it a hover away. -->
  <div class="title" data-tauri-drag-region>
    {#if agent}
      <span class="name" title={agent.task.prompt} data-tauri-drag-region>
        {#if projectName}
          <span class="project">{projectName}</span>
          <span class="sep">/</span>
        {/if}
        {store.agentName(agent)}
      </span>
    {:else if drafting}
      <span class="name" data-tauri-drag-region>New agent</span>
    {:else}
      <span class="idle" data-tauri-drag-region>No agent selected</span>
    {/if}
  </div>

  {#if ownsWindowFrame}
    <div class="controls">
      <button class="ctl" onclick={() => win.minimize()} aria-label="Minimize">
        <svg viewBox="0 0 10 10" aria-hidden="true"><path d="M0 5h10" /></svg>
      </button>
      <button
        class="ctl"
        onclick={() => win.toggleMaximize()}
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
      <button class="ctl close" onclick={() => win.close()} aria-label="Close">
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
     are — at the small end of the root font scale the bar could clip them. */
  .chrome.mac {
    height: max(var(--titlebar-h), 28px);
  }

  .lead {
    flex: none;
    display: flex;
    align-items: center;
    padding: 0 var(--pad-x);
    background: var(--panel-bg);
    overflow: hidden;
  }

  /* The traffic lights live in this corner, so the title starts after them. */
  .chrome.mac .lead {
    padding-left: 5rem;
  }

  .wordmark {
    /* Stands in for the app icon until there is one, so it carries more weight
       than a caption would. Sized to sit comfortably in the bar. */
    font-size: var(--text-2xl);
    line-height: var(--leading-none);
    font-weight: var(--weight-normal);
    letter-spacing: 0.01em;
    color: var(--fg-muted);
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .wordmark b {
    font-weight: var(--weight-bold);
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
    font-size: var(--text-md);
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

  .idle {
    font-size: var(--text-md);
    color: var(--fg-muted);
    font-style: italic;
  }

  .controls {
    display: flex;
    align-items: stretch;
    flex: none;
    margin-left: auto;
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
