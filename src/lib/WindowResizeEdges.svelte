<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { ownsWindowFrame } from "./platform";

  /**
   * Grab strips around an undecorated window.
   *
   * Only Windows runs undecorated now (see `platform.ts`), and taking the
   * decoration away there takes the OS resize frame with it, so the app brings
   * its own — thin enough that the scrollbar underneath the right edge stays
   * usable, and gone while the window reports itself maximized, when there is
   * nothing to resize. Everywhere else the OS or the compositor still owns the
   * frame and does its own resizing, and `ownsWindowFrame` keeps these off.
   */
  const win = getCurrentWindow();

  // The API takes this as a string union it doesn't export, so borrow it back
  // off the method rather than restating the eight names as a second source.
  type ResizeDirection = Parameters<typeof win.startResizeDragging>[0];

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

  const EDGES: readonly (readonly [string, ResizeDirection])[] = [
    ["n", "North"],
    ["s", "South"],
    ["e", "East"],
    ["w", "West"],
    ["ne", "NorthEast"],
    ["nw", "NorthWest"],
    ["se", "SouthEast"],
    ["sw", "SouthWest"],
  ];

  function grab(e: PointerEvent, direction: ResizeDirection) {
    if (e.button !== 0) return;
    e.preventDefault();
    win.startResizeDragging(direction);
  }
</script>

{#if ownsWindowFrame && !maximized}
  {#each EDGES as [edge, direction] (edge)}
    <div
      class="edge {edge}"
      role="presentation"
      onpointerdown={(e) => grab(e, direction)}
    ></div>
  {/each}
{/if}

<style>
  .edge {
    position: fixed;
    /* Above the usage overlay's backdrop, so the window stays resizable while
       a modal is open. */
    z-index: var(--z-modal);
  }

  /* Sides: thin, so they cost the content underneath as little as possible. */
  .n,
  .s {
    left: 0;
    right: 0;
    height: 3px;
    cursor: ns-resize;
  }
  .e,
  .w {
    top: 0;
    bottom: 0;
    width: 3px;
    cursor: ew-resize;
  }
  .n {
    top: 0;
  }
  .s {
    bottom: 0;
  }
  .e {
    right: 0;
  }
  .w {
    left: 0;
  }

  /* Corners: bigger, because that's where a resize is usually aimed. */
  .ne,
  .nw,
  .se,
  .sw {
    width: 12px;
    height: 12px;
  }
  .nw {
    top: 0;
    left: 0;
    cursor: nwse-resize;
  }
  .ne {
    top: 0;
    right: 0;
    cursor: nesw-resize;
  }
  .sw {
    bottom: 0;
    left: 0;
    cursor: nesw-resize;
  }
  .se {
    bottom: 0;
    right: 0;
    cursor: nwse-resize;
  }
</style>
