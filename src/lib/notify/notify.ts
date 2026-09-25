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
import { api } from "$lib/api";
import type { Notice } from "./turnNotice";

/** Asked once per launch; null until then. */
let allowed: Promise<boolean> | null = null;

function permission(): Promise<boolean> {
  allowed ??= isPermissionGranted()
    .then((granted) => granted || requestPermission().then((p) => p === "granted"))
    .catch(() => false);
  return allowed;
}

/**
 * The app's logo as a file, where the OS won't find it on its own (see
 * `notification_icon`). Fetched once per launch; undefined leaves the OS
 * to its default.
 */
let logo: Promise<string | undefined> | null = null;

function icon(): Promise<string | undefined> {
  logo ??= api
    .notificationIcon()
    .then((path) => path ?? undefined)
    .catch(() => undefined);
  return logo;
}

export async function notify(notice: Notice, onScreen: boolean) {
  if (onScreen && document.hasFocus()) return;
  if (!(await permission())) return;
  sendNotification({ ...notice, icon: await icon() });
}
