/**
 * The Spaces (ADR 0017), in the order the rail shows them. This list is the
 * whole of what the rail, the phone's tab bar, the window and the shortcuts
 * know about a Space, so adding one is an entry here and its id in `SpaceId`.
 */

import type { Component } from "svelte";
import AgentsSpace from "$lib/agent/AgentsSpace.svelte";
import CalendarSpace from "$lib/calendar/CalendarSpace.svelte";
import MailSpace from "$lib/mail/MailSpace.svelte";
import NotesSpace from "$lib/notes/NotesSpace.svelte";
import type { SpaceId } from "./space.svelte";

export type Space = {
  id: SpaceId;
  /** What the rail's tooltip and the phone's tab say. */
  label: string;
  /**
   * Line art on a 20-unit grid, one `d` per stroke. The rail draws them all
   * the same way — 1.5 wide, round ends, no fill — so every icon matches.
   */
  icon: string[];
  /**
   * Fills the box to the right of the rail, and stays mounted once opened so
   * it comes back as it was left. It takes no props. Mounted isn't showing,
   * so keys it listens for on the window are only its own while
   * `space.current` is its id.
   */
  component: Component;
  /** The digit that, held with Ctrl (⌘ on a Mac), switches to it. */
  shortcut: string;
};

export const SPACES: Space[] = [
  {
    id: "agents",
    label: "Agents",
    // A terminal's prompt: what an Agent is, a `claude` at work.
    icon: [
      "M5 3.5h10a2.5 2.5 0 0 1 2.5 2.5v8a2.5 2.5 0 0 1-2.5 2.5H5A2.5 2.5 0 0 1 2.5 14V6A2.5 2.5 0 0 1 5 3.5z",
      "M6 7.75 8.5 10 6 12.25",
      "M10.5 12.5h3.5",
    ],
    component: AgentsSpace,
    shortcut: "1",
  },
  {
    id: "notes",
    label: "Notes",
    icon: [
      "M11.5 2.5H6a2 2 0 0 0-2 2v11a2 2 0 0 0 2 2h8a2 2 0 0 0 2-2V7z",
      "M11.5 2.5V7H16",
      "M7 10.5h6",
      "M7 13.5h4",
    ],
    component: NotesSpace,
    shortcut: "2",
  },
  {
    id: "calendar",
    label: "Calendar",
    icon: [
      "M5 4.5h10a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-9a2 2 0 0 1 2-2z",
      "M3 8.5h14",
      "M7 2.5v3.5",
      "M13 2.5v3.5",
      "M7 12h.01M10 12h.01M13 12h.01M7 14.75h.01M10 14.75h.01",
    ],
    component: CalendarSpace,
    shortcut: "3",
  },
  {
    id: "mail",
    label: "Mail",
    icon: [
      "M4.5 4.5h11a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2h-11a2 2 0 0 1-2-2v-7a2 2 0 0 1 2-2z",
      "M3 6l7 5.25L17 6",
    ],
    component: MailSpace,
    shortcut: "4",
  },
];

/**
 * The Space a key press asks for, if it's one of the shortcuts: Ctrl or ⌘,
 * but not both, and nothing else held. Read off `code`, not `key`, so it's
 * the digit key wherever the layout puts the digit itself behind Shift.
 */
export function spaceForKey(
  e: Pick<KeyboardEvent, "code" | "ctrlKey" | "metaKey" | "altKey" | "shiftKey">,
  spaces: readonly Pick<Space, "id" | "shortcut">[] = SPACES,
): SpaceId | null {
  if (e.ctrlKey === e.metaKey || e.altKey || e.shiftKey) return null;
  return spaces.find((s) => e.code === `Digit${s.shortcut}`)?.id ?? null;
}

/**
 * The stylesheet that shows the current Space, before the app has started as
 * much as after. The pre-paint script in `app.html` puts the last Space on
 * `<html data-space>`, but the page is rendered ahead of time as Agents (ADR
 * 0015), so it can't be the markup that says which box to show or which tab to
 * light. These rules match `<html data-space>` against each box and tab
 * instead: the box shows, and the tab gets `--current: 1` for its own styles
 * to read.
 */
export function currentSpaceCss(spaces: readonly Pick<Space, "id">[] = SPACES): string {
  const each = (sel: (id: string) => string) =>
    spaces.map(({ id }) => `html[data-space="${id}"] ${sel(id)}`).join(",");
  return (
    `${each((id) => `[data-space-box="${id}"]`)}{visibility:visible}` +
    `${each((id) => `[data-space-tab="${id}"]`)}{--current:1}`
  );
}
