/**
 * The window's size, as reactive state, plus the layout mode derived from it.
 *
 * Most of the responsive work is plain CSS; this exists for the one thing CSS
 * can't decide on its own — how the project pane gives up width when the
 * window runs out of it.
 */

/**
 * Below this, the window can't hold a readable tree and a readable detail pane
 * at the same time, so the tree shrinks to a rail of initials. It never leaves:
 * switching projects and agents is the app's main move, and a navigation pane
 * you have to go back to isn't navigation.
 */
const RAIL_AT = 520;

class Viewport {
  width = $state(1280);
  height = $state(800);

  private stopFn: (() => void) | null = null;
  private frame = 0;

  /** True once the window is too tight for the tree at a readable width. */
  railed = $derived(this.width < RAIL_AT);

  /** Begin tracking. Safe to call more than once. */
  start() {
    if (this.stopFn || typeof window === "undefined") return;

    const apply = () => {
      this.frame = 0;
      this.width = window.innerWidth;
      this.height = window.innerHeight;
    };

    // A window drag produces size changes faster than the screen repaints, so
    // coalesce them: one measurement per frame, always the latest one.
    const schedule = () => {
      if (this.frame) return;
      this.frame = requestAnimationFrame(apply);
    };

    apply();

    // `resize` on its own is unreliable mid-drag — webviews are free to hold it
    // back until the drag ends, which is exactly when the panes look frozen.
    // A ResizeObserver reports from layout instead, so it fires every frame the
    // window actually changes size.
    const ro = new ResizeObserver(schedule);
    ro.observe(document.documentElement);
    window.addEventListener("resize", schedule);

    this.stopFn = () => {
      window.removeEventListener("resize", schedule);
      ro.disconnect();
      if (this.frame) cancelAnimationFrame(this.frame);
      this.frame = 0;
    };
  }

  stop() {
    this.stopFn?.();
    this.stopFn = null;
  }
}

export const viewport = new Viewport();
