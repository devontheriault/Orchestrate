import type { HostInfo, HostStatus } from "$lib/api";

/**
 * What to tell the user about the window's link to this machine's Host, or
 * null when there's nothing to say. (Other machines' Hosts say how they are
 * beside their agents instead: one being off is ordinary.) Reconnecting is quiet for a moment first
 * (`HOST_NOTICE_DELAY`), since a Host restarting for an update is back
 * before anyone would read the banner.
 */
export function hostNotice(status: HostStatus): string | null {
  switch (status.state) {
    case "connected":
      return null;
    case "connecting":
      return status.error
        ? `Can't reach the Host: ${status.error}. Retrying…`
        : "Reconnecting to the Host…";
    case "updating":
      return `The Host is still on version ${status.version}. It will update once its running agents finish, and this window reconnects then.`;
    case "outdated":
      return `The Host runs a newer version (${status.version}). Update this app to reach your agents.`;
    // Only another machine's Host can be behind this window or refuse it.
    case "behind":
    case "refused":
      return null;
  }
}

/**
 * What to tell the user on a phone, which has no Host of its own: only that
 * none of the Hosts added can be reached, most often because Tailscale is off
 * here. One machine being off among several is ordinary, and shown beside its
 * agents; having added none is the project list's to say.
 */
export function phoneNotice(list: HostInfo[]): string | null {
  if (!list.length || list.some((h) => h.status.state === "connected")) return null;
  return "Can't reach any of your machines. Check that Tailscale is connected on this device.";
}

export const HOST_NOTICE_DELAY = 1500;
