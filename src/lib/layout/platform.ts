import type { Window } from "@tauri-apps/api/window";

/**
 * Which window frame this build is wearing.
 *
 * `build_main_window` in `src-tauri/src/lib.rs` decides it: macOS keeps its
 * decorations (native traffic lights, hidden title) while every other platform
 * runs undecorated. The UI has to match that choice — reserve room for the
 * traffic lights on macOS, draw its own controls and resize edges everywhere
 * else.
 *
 * The one exception is a tiling compositor, which owns every window's frame
 * without drawing one. The backend flags that with an initialization script,
 * and a phone the same way: the app fills the screen and has no frame at all.
 *
 * The pre-paint script in `src/app.html` works out which it is and writes it to
 * `<html data-frame>`, because the page arrives already rendered — without
 * knowing the platform — and the stylesheet has to fit the header to the frame
 * before the first paint. This reads it back rather than asking again. While
 * rendering ahead of time there is no document, and the app's own frame is
 * assumed.
 */
type Frame = "mac" | "compositor" | "mobile" | "app";

const frame: Frame =
  typeof document === "undefined"
    ? "app"
    : ((document.documentElement.dataset.frame as Frame | undefined) ?? "app");

/** True where the app, not the OS or the compositor, owns the window frame. */
export const ownsWindowFrame = frame === "app";

/**
 * True on a phone or tablet. It runs no Host of its own — it can't run
 * `claude` — so the window there only reaches other machines' Hosts.
 */
export const mobile = frame === "mobile";

/**
 * The header's `data-tauri-drag-region` value. Pressing on it starts a move (or
 * a maximize on double-click) everywhere except under a tiling compositor. There
 * a window goes where the layout puts it, and a drag pulls it out of the tile.
 * A phone's window doesn't move at all.
 * Tauri reads `"false"` as "no drag region", so the attribute stays on the element.
 */
export const dragRegion = frame === "compositor" || mobile ? "false" : "true";

/**
 * Keep `set` told whether `win` is maximized: now, and after every resize.
 * Only asked where the app draws the frame — the maximize glyph and the resize
 * edges both depend on it. Returns the cleanup, for the `$effect` that calls it
 * to hand back.
 */
export function watchMaximized(win: Window, set: (maximized: boolean) => void): () => void {
  let live = true;
  let unlisten: (() => void) | undefined;
  const sync = () => {
    win.isMaximized().then((m) => {
      if (live) set(m);
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
}
