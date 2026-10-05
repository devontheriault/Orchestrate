/**
 * Which Space the window is showing (ADR 0017), remembered across launches.
 *
 * `<html data-space>` is what the stylesheet reads to show it, and the
 * pre-paint script in `src/app.html` sets it from the same key before the
 * first frame. This keeps the two in step from then on.
 *
 * It holds no list of Spaces: `spaces.ts` imports every Space's component,
 * and the store, which a Space's component reads, asks this for Agents.
 */

import { SvelteSet } from "svelte/reactivity";

export type SpaceId = "agents" | "notes" | "calendar" | "mail";

const STORAGE_KEY = "orchestrate:space";

/** Where a window with nothing stored opens. */
export const DEFAULT_SPACE: SpaceId = "agents";

class CurrentSpace {
  current = $state<SpaceId>(DEFAULT_SPACE);

  /**
   * The Spaces opened since launch. Each stays mounted once opened, so leaving
   * one loses nothing: the selected Agent, the transcript's scroll and the
   * composer's draft are all as they were on the way back. Agents is always
   * here, because the page is rendered ahead of time with it.
   */
  readonly opened = new SvelteSet<SpaceId>([DEFAULT_SPACE]);

  constructor() {
    if (typeof localStorage === "undefined") return;
    try {
      const stored = localStorage.getItem(STORAGE_KEY);
      if (stored) this.current = stored as SpaceId;
    } catch {
      // Storage being unavailable just means opening on Agents.
    }
    this.opened.add(this.current);
  }

  /**
   * Called once, from the page, with the Spaces there are. A stored Space
   * that's gone from the list since opens Agents instead.
   */
  start(known: readonly SpaceId[]) {
    if (!known.includes(this.current)) this.show(DEFAULT_SPACE);
  }

  show(id: SpaceId) {
    this.opened.add(id);
    if (this.current === id) return;
    this.current = id;
    document.documentElement.dataset.space = id;
    try {
      localStorage.setItem(STORAGE_KEY, id);
    } catch {
      // Not remembering it shouldn't stop the switch.
    }
  }
}

export const space = new CurrentSpace();
