/**
 * The window's size, as reactive state, plus the layout mode derived from it.
 *
 * Most of the responsive work is plain CSS; this exists for the parts that
 * can't be done in CSS alone — dropping a pane from the DOM when the window is
 * too narrow to hold both at a usable width.
 */

/** Below this, only one of (project tree, detail) is on screen at a time. */
const NARROW_AT = 760;

class Viewport {
  width = $state(1280);
  height = $state(800);

  private stopFn: (() => void) | null = null;

  /**
   * Too narrow for the tree and the detail pane side by side. Above it both
   * fit: the tree carries the agents now, so it earns its width at any size
   * the window can spare it, and only a hand-dragged pane becomes a rail.
   */
  narrow = $derived(this.width < NARROW_AT);

  /** Begin tracking. Safe to call more than once. */
  start() {
    if (this.stopFn || typeof window === "undefined") return;
    const measure = () => {
      this.width = window.innerWidth;
      this.height = window.innerHeight;
    };
    measure();
    window.addEventListener("resize", measure);
    this.stopFn = () => window.removeEventListener("resize", measure);
  }

  stop() {
    this.stopFn?.();
    this.stopFn = null;
  }
}

export const viewport = new Viewport();
