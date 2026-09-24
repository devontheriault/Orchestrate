/**
 * Tell the user, through the OS, that an agent needs them (see
 * `turnNotice`). Skipped when they're already looking at that agent in a
 * focused window: they can see it for themselves.
 */

import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import type { Notice } from "./turnNotice";

/** Asked once per launch; null until then. */
let allowed: Promise<boolean> | null = null;

function permission(): Promise<boolean> {
  allowed ??= isPermissionGranted()
    .then((granted) => granted || requestPermission().then((p) => p === "granted"))
    .catch(() => false);
  return allowed;
}

export async function notify(notice: Notice, onScreen: boolean) {
  if (onScreen && document.hasFocus()) return;
  if (!(await permission())) return;
  sendNotification(notice);
}
