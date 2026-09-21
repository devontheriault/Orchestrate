/**
 * The user-chosen width of the project pane.
 *
 * Until the user drags the divider the width is `null`, meaning "whatever the
 * stylesheet says" — the viewport-relative `clamp()` default in +layout.svelte.
 * Once dragged, the width becomes an explicit pixel value that overrides the
 * default and is remembered across restarts.
 */

const STORAGE_KEY = "devcode:panes";

/** Below this the project list is drawn as a rail of initials, not a tree. */
export const MIN_PROJECTS = 150;
/** A rail is still draggable, so the floor sits below the collapse point. */
export const MIN_PROJECTS_DRAG = 40;
/** The detail pane flexes, but never below this. */
export const MIN_DETAIL = 280;

class Panes {
  projects = $state<number | null>(null);

  constructor() {
    if (typeof localStorage === "undefined") return;
    try {
      const raw = localStorage.getItem(STORAGE_KEY);
      if (!raw) return;
      const v = JSON.parse(raw) as { projects?: number | null };
      this.projects = typeof v.projects === "number" ? v.projects : null;
    } catch {
      // A corrupt entry just means "use the defaults".
    }
  }

  private save() {
    if (typeof localStorage === "undefined") return;
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify({ projects: this.projects }));
    } catch {
      // Storage being unavailable shouldn't break resizing.
    }
  }

  setProjects(w: number | null) {
    this.projects = w;
    this.save();
  }

  /**
   * Give back the width the project pane should render at for a window this
   * wide, shrinking the user's choice rather than letting it squeeze the detail
   * pane out of the window.
   */
  fit(windowWidth: number) {
    let projects = this.projects;

    // A pane still on its default takes a share of the window, so reserve the
    // same share the stylesheet would.
    const p = projects ?? clamp(windowWidth * 0.2, 176, 320);
    const over = p + MIN_DETAIL - windowWidth;

    if (over > 0 && projects !== null) {
      projects = Math.max(MIN_PROJECTS_DRAG, p - over);
    }

    return { projects };
  }
}

function clamp(v: number, lo: number, hi: number) {
  return Math.min(hi, Math.max(lo, v));
}

export const panes = new Panes();
