/**
 * The window's size, as reactive state, plus the layout mode derived from it.
 *
 * Most of the responsive work is plain CSS; this exists for the parts that
 * can't be done in CSS alone — dropping a pane from the DOM when the window is
 * too narrow to hold all three at a usable width.
 */

/** Below this, the project list collapses to a rail of initials. */
const COMPACT_AT = 1080;
/** Below this, only one of (agent list, detail) is on screen at a time. */
const NARROW_AT = 760;

export type Layout = "wide" | "compact" | "narrow";

class Viewport {
  width = $state(1280);
  height = $state(800);

  private stopFn: (() => void) | null = null;

  layout = $derived<Layout>(
    this.width < NARROW_AT ? "narrow" : this.width < COMPACT_AT ? "compact" : "wide",
  );

  /** True once the window is too narrow for a full project list. */
  railed = $derived(this.layout !== "wide");

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
