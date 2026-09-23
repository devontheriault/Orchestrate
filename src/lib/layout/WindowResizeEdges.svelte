<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { ownsWindowFrame, watchMaximized } from "./platform";

  /**
   * Grab strips around an undecorated window.
   *
   * Taking the decoration away takes the OS resize frame with it on Windows,
   * and on X11 desktops that don't add one back, so the app brings its own —
   * thin enough that the scrollbar underneath the right edge stays usable, and
   * gone while the window reports itself maximized, when there's nothing to
   * resize. Under a tiling compositor, which does the resizing itself,
   * `ownsWindowFrame` keeps them off entirely.
   */
  const win = getCurrentWindow();

  // The API takes this as a string union it doesn't export, so borrow it back
  // off the method rather than restating the eight names as a second source.
  type ResizeDirection = Parameters<typeof win.startResizeDragging>[0];

  let maximized = $state(false);

  $effect(() => {
    if (!ownsWindowFrame) return;
    return watchMaximized(win, (m) => (maximized = m));
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
