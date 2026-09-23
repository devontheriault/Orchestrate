/**
 * Which window frame this build is wearing.
 *
 * `build_main_window` in `src-tauri/src/lib.rs` decides it: macOS keeps its
 * decorations (native traffic lights, hidden title) while every other platform
 * runs undecorated. The UI has to match that choice — reserve room for the
 * traffic lights on macOS, draw its own controls and resize edges everywhere
 * else — and reads it off the user agent rather than an async `invoke`, so the
 * header renders right on the first frame instead of shifting once an IPC call
 * lands.
 *
 * The one exception is a tiling compositor, which owns every window's frame
 * without drawing one. The backend flags that with an initialization script
 * that runs before this module, so it's just as synchronous.
 */
export const isMac =
  typeof navigator !== "undefined" && /Mac OS X|Macintosh/.test(navigator.userAgent);

const compositorOwnsFrame =
  typeof window !== "undefined" &&
  (window as { __COMPOSITOR_OWNS_FRAME__?: boolean }).__COMPOSITOR_OWNS_FRAME__ === true;

/** True where the app, not the OS or the compositor, owns the window frame. */
export const ownsWindowFrame = !isMac && !compositorOwnsFrame;
