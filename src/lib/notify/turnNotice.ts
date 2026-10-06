import type { Agent } from "$lib/api";

export type Notice = { title: string; body: string };

/**
 * What to tell the user about an agent whose record just changed, or null
 * for nothing. Only a Turn ending in a way that needs them says anything:
 *
 * - Completed with nothing queued: it's waiting on them. One that Completes
 *   with more queued never reaches here — the Host starts the next Turn
 *   before announcing the first one's end.
 * - Completed with its queue still full: the next message couldn't be sent,
 *   so the queue is held.
 * - Failed: a failed Turn is a thing to read, and any queue is held behind it.
 *
 * A Stop is the user's own doing, and an Orphan is the Host going away; a
 * rename or a queue edit on an idle agent isn't a Turn ending at all. A Helper
 * that Completes is its Lead's to hear about, not the user's (ADR 0019).
 */
export function turnNotice(before: Agent | undefined, after: Agent, name: string): Notice | null {
  if (before?.state !== "running" || after.state === "running") return null;
  const held = after.queue?.length ?? 0;
  const heldNote = held
    ? ` ${held} queued message${held === 1 ? " is" : "s are"} held.`
    : "";
  switch (after.state) {
    case "completed":
      if (after.lead_id && !held) return null;
      return held
        ? {
            title: `${name}: queue held`,
            body: `The next queued message couldn't be sent.${heldNote}`,
          }
        : { title: `${name} completed`, body: "Ready for your next message." };
    case "failed": {
      const why = after.fail_reason?.split("\n")[0]?.trim();
      return {
        title: `${name} failed`,
        body: `${why || "The Turn ended with an error."}${heldNote}`,
      };
    }
    default:
      return null;
  }
}
