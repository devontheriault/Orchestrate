/**
 * The user-chosen width of the project pane.
 *
 * Until the user drags the divider the width is `null`, meaning "whatever the
 * stylesheet says" — the viewport-relative `clamp()` default in +layout.svelte.
 * Once dragged, the width becomes an explicit pixel value that overrides the
 * default and is remembered across restarts.
 */

import { viewport } from "./viewport.svelte";

const STORAGE_KEY = "orchestrate:panes";

/** Below this the project list is drawn as a rail of initials, not a tree. */
export const MIN_PROJECTS = 150;
/**
 * A rail is still draggable, so the floor sits below the collapse point — but
 * not below the rail's own natural width (`--rail`, 3.4rem at the largest root
 * font size). Anything narrower would be a stretch of travel where the pointer
 * moves and the seam can't follow it, which reads as a stuck drag.
 */
export const MIN_PROJECTS_DRAG = 56;
/** The detail pane flexes, but never below this. */
export const MIN_DETAIL = 280;

/** Long enough to outlast a drag, short enough to survive a sudden quit. */
const SAVE_DEBOUNCE = 200;

class Panes {
  projects = $state<number | null>(null);

  /** The width the project pane renders at, or null while on its default. */
  width = $derived(this.fit(viewport.width).projects);

  /** True when the project pane is drawn as a rail of initials. */
  railed = $derived(
    this.width === null ? viewport.railed : this.width < MIN_PROJECTS,
  );

  /**
   * The same width as a CSS length, for anything outside the pane that has to
   * line up with its seam — the title bar's lead segment, for one.
   */
  cssWidth = $derived(
    this.width !== null
      ? `${this.width}px`
      : this.railed
        ? "var(--rail)"
        : "var(--pane-projects)",
  );

  private timer: ReturnType<typeof setTimeout> | null = null;

  constructor() {
    if (typeof localStorage === "undefined") return;
    try {
      const raw = localStorage.getItem(STORAGE_KEY);
      if (raw) {
        const v = JSON.parse(raw) as { projects?: number | null };
        this.projects =
          typeof v.projects === "number"
            ? Math.max(MIN_PROJECTS_DRAG, v.projects)
            : null;
      }
    } catch {
      // A corrupt entry just means "use the defaults".
    }

    // The debounce below can still be in flight when the window goes away.
    if (typeof window !== "undefined") {
      window.addEventListener("pagehide", () => this.flush());
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

  /**
   * Write the width out, but not on the drag's critical path.
   *
   * `localStorage` is synchronous, and a drag sets the width once per pointer
   * event. Saving inline meant a blocking write between every pair of frames,
   * which is what made the seam stutter and lag behind the cursor.
   */
  private queueSave() {
    if (this.timer) clearTimeout(this.timer);
    this.timer = setTimeout(() => {
      this.timer = null;
      this.save();
    }, SAVE_DEBOUNCE);
  }

  /** Commit a pending write immediately. */
  flush() {
    if (!this.timer) return;
    clearTimeout(this.timer);
    this.timer = null;
    this.save();
  }

  /**
   * Keep `--pane-projects` and `--rail` on `<html>` at the width, where the
   * pre-paint script in `src/app.html` puts the stored one before the app runs;
   * with neither set, the stylesheet's defaults stand. Called once, from the
   * page.
   *
   * A hand-sized pane keeps its width once it collapses, so the seam carries on
   * tracking the pointer through the rail instead of snapping to the stylesheet
   * rail and sitting there for the rest of the drag. An untouched pane still
   * gets the rail the stylesheet picked.
   */
  start() {
    $effect(() => {
      const root = document.documentElement.style;
      if (this.width === null) {
        root.removeProperty("--pane-projects");
        root.removeProperty("--rail");
      } else {
        root.setProperty("--pane-projects", `${this.width}px`);
        root.setProperty("--rail", `${this.width}px`);
      }
    });
  }

  setProjects(w: number | null) {
    if (this.projects === w) return;
    this.projects = w;
    this.queueSave();
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
