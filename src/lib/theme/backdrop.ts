import { Effect, EffectState, getCurrentWindow } from "@tauri-apps/api/window";

/**
 * Ask the OS to blur what is behind the window, or to stop — the half of the
 * Glass theme the page can't do, because a webview only sees what it paints.
 *
 * macOS gets its HUD material and Windows 10/11 Acrylic, the first of the two
 * each one supports. Acrylic is only asked for while Glass is on because it
 * makes dragging and resizing the window lag. macOS can't take its material
 * back off, which is harmless: every other theme paints over it.
 *
 * Linux has no call for this: blur there is the compositor's, and is on where
 * the user has turned it on (Hyprland's `decoration:blur`, KDE's Blur effect).
 */
export function blurBehindWindow(on: boolean): void {
  const win = getCurrentWindow();
  const done = on
    ? win.setEffects({ effects: [Effect.HudWindow, Effect.Acrylic], state: EffectState.Active })
    : win.clearEffects();
  done.catch((e) => console.warn("window blur:", e));
}
