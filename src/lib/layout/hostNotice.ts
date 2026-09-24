import type { HostStatus } from "$lib/api";

/**
 * What to tell the user about the window's link to its Host, or null when
 * there's nothing to say. Reconnecting is quiet for a moment first
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
  }
}

export const HOST_NOTICE_DELAY = 1500;
