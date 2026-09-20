/**
 * User-chosen widths for the two fixed panes (projects, agent list).
 *
 * Until the user drags a divider a pane's width is `null`, meaning "whatever
 * the stylesheet says" — the viewport-relative `clamp()` defaults in
 * +layout.svelte. Once dragged, the width becomes an explicit pixel value that
 * overrides the default and is remembered across restarts.
 */

const STORAGE_KEY = "devcode:panes";

/** Below this the project list is drawn as a rail of initials, not a list. */
export const MIN_PROJECTS = 150;
/** A rail is still draggable, so the floor sits below the collapse point. */
export const MIN_PROJECTS_DRAG = 40;
export const MIN_AGENTS = 190;
/** The detail pane flexes, but never below this. */
export const MIN_DETAIL = 280;

type Stored = { projects: number | null; agents: number | null };

class Panes {
  projects = $state<number | null>(null);
  agents = $state<number | null>(null);

  constructor() {
    if (typeof localStorage === "undefined") return;
    try {
      const raw = localStorage.getItem(STORAGE_KEY);
      if (!raw) return;
      const v = JSON.parse(raw) as Partial<Stored>;
      this.projects = typeof v.projects === "number" ? v.projects : null;
      this.agents = typeof v.agents === "number" ? v.agents : null;
    } catch {
      // A corrupt entry just means "use the defaults".
    }
  }

  private save() {
    if (typeof localStorage === "undefined") return;
    try {
      localStorage.setItem(
        STORAGE_KEY,
        JSON.stringify({ projects: this.projects, agents: this.agents }),
      );
    } catch {
      // Storage being unavailable shouldn't break resizing.
    }
  }

  setProjects(w: number | null) {
    this.projects = w;
    this.save();
  }

  setAgents(w: number | null) {
    this.agents = w;
    this.save();
  }

  /**
   * Give back the widths the panes should render at for a window this wide,
   * shrinking the user's choices (agent list first) rather than letting them
   * squeeze the detail pane out of the window.
   */
  fit(windowWidth: number, hasAgentPane: boolean) {
    let projects = this.projects;
    let agents = hasAgentPane ? this.agents : null;

    // A pane still on its default takes a share of the window, so reserve the
    // same share the stylesheet would.
    const defaultProjects = clamp(windowWidth * 0.15, 160, 256);
    const defaultAgents = clamp(windowWidth * 0.23, 208, 352);

    let p = projects ?? defaultProjects;
    let a = hasAgentPane ? (agents ?? defaultAgents) : 0;
    let over = p + a + MIN_DETAIL - windowWidth;

    if (over > 0 && agents !== null) {
      const give = Math.min(over, a - MIN_AGENTS);
      if (give > 0) {
        a -= give;
        over -= give;
        agents = a;
      }
    }
    if (over > 0 && projects !== null) {
      projects = Math.max(MIN_PROJECTS_DRAG, p - over);
    }

    return { projects, agents };
  }
}

function clamp(v: number, lo: number, hi: number) {
  return Math.min(hi, Math.max(lo, v));
}

export const panes = new Panes();
