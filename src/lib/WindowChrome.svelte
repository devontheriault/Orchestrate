<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { isMac, ownsWindowFrame } from "./platform";
  import { panes } from "./panes.svelte";

  const win = getCurrentWindow();

  // Drives the maximize/restore glyph. Only tracked where this app draws the
  // button — macOS has its own and never asks.
  let maximized = $state(false);

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
    <span class="wordmark" data-tauri-drag-region>DevCode</span>
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
    z-index: 200;
  }

  /* Leave the traffic lights their corner, and never get shorter than they
     are — at the small end of the root font scale 2.1rem would clip them. */
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
    font-size: 0.8rem;
    font-weight: 600;
    letter-spacing: 0.01em;
    color: var(--fg-muted);
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
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
    background: #e81123;
    color: #fff;
  }
</style>
