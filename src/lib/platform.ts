/**
 * Which window frame this build is wearing.
 *
 * `build_main_window` in `src-tauri/src/lib.rs` decides it at compile time, and
 * the rule there is to wear whatever frame the desktop hands out. macOS and
 * Linux both keep their decorations — on Linux GTK negotiates with the
 * compositor and takes server-side decorations wherever they're offered, which
 * under a tiling compositor means no titlebar at all — so on both the OS owns
 * the frame and the app must not draw window buttons or resize edges over it.
 * Windows alone runs undecorated and supplies its own.
 *
 * The UI reads this off the user agent rather than an async `invoke`, so the
 * header renders right on the first frame instead of shifting once an IPC call
 * lands.
 */
const ua = typeof navigator !== "undefined" ? navigator.userAgent : "";

/** Drives the room the header leaves for the traffic lights. */
export const isMac = /Mac OS X|Macintosh/.test(ua);

/**
 * True where the app, not the OS, owns the window frame — so it has to draw the
 * minimize/maximize/close controls and the resize edges itself.
 */
export const ownsWindowFrame = /Windows NT/.test(ua);
