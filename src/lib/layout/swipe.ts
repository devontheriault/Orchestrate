/**
 * Sideways swipes on a phone, followed by the finger: back out of a screen,
 * over to the next Space, on to the next day. A webview gets none of these
 * from the OS, so the phone layout reads them off the touches itself.
 *
 * Swipes nest: a Calendar's grid sits in a Space, which sits beside the other
 * Spaces. The innermost one that takes a touch has it, and anything that
 * scrolls sideways, or is being typed in, has it before any of them.
 */

export type Point = { x: number; y: number };

/** -1 is a swipe to the left, toward what's next; 1 to the right, back. */
export type Dir = -1 | 1;

/** How near the left edge a swipe has to start to count as iOS's own back swipe. */
export const EDGE = 24;

/** How far a finger moves before it's said to be going one way or the other. */
const SLOP = 10;

/** Past this share of the width, a swipe let go of goes through. */
const HALFWAY = 0.4;

/** A flick this fast (px/ms) goes through however short it was. */
const FLICK = 0.35;

/**
 * Which way a touch from `from` to `to` is going, once it has gone far enough
 * to say: sideways only when it's well more across than down, so a scroll
 * that wanders never turns into a swipe.
 */
export function axisOf(from: Point, to: Point): "x" | "y" | null {
  const dx = Math.abs(to.x - from.x);
  const dy = Math.abs(to.y - from.y);
  if (Math.max(dx, dy) < SLOP) return null;
  return dx > dy * 1.5 ? "x" : "y";
}

/**
 * Whether a swipe let go of `dx` across a `width`-wide screen, moving at
 * `velocity`, goes through: far enough, or flicked its own way.
 */
export function goesThrough(dx: number, width: number, velocity: number): boolean {
  if (dx === 0) return false;
  const sign = Math.sign(dx);
  if (velocity * sign < -FLICK / 2) return false;
  return Math.abs(dx) > width * HALFWAY || velocity * sign > FLICK;
}

/** How far to draw a pull that has nowhere to go: less the further it goes. */
export function resist(dx: number): number {
  return Math.sign(dx) * Math.sqrt(Math.abs(dx)) * 4;
}

/** How long to finish a swipe in, given the distance left and how fast it was going. */
export function settleMs(left: number, velocity: number): number {
  const speed = Math.max(Math.abs(velocity), 1.2);
  return Math.round(Math.min(320, Math.max(160, Math.abs(left) / speed)));
}

/** Whether the user asked the OS for less motion: every swipe then lands at once. */
export function reducedMotion(): boolean {
  return typeof matchMedia !== "undefined" && matchMedia("(prefers-reduced-motion: reduce)").matches;
}

export type Swipe = {
  /** Whether it can go `dir` from here, now. A swipe it can't take is left to the one around it. */
  can(dir: Dir, from: Point): boolean;
  /** The finger has moved: `dx` from where it went down. */
  move(dx: number): void;
  /** Let go at `dx`, moving at `velocity` px/ms. */
  end(dx: number, velocity: number): void;
  /**
   * While true, every touch begun inside is this one's, even a swipe it
   * can't take: a screen pushed over a list is no place to switch Spaces from.
   */
  holds?: () => boolean;
};

/** Touches already had by a swipe nested deeper, by their `touchstart` or `touchmove`. */
const had = new WeakSet<Event>();

/**
 * Whether something between `target` and `root` gets this swipe first: a
 * field being typed in (unless it starts at the edge, which is always the
 * way back), or anything scrolled sideways that can scroll further `dir`.
 */
function wantedInside(target: EventTarget | null, root: Element, dir: Dir, from: Point): boolean {
  for (let el = target instanceof Element ? target : null; el && el !== root; el = el.parentElement) {
    if (el instanceof HTMLElement) {
      if (from.x > EDGE && (el.isContentEditable || el.matches("input, textarea, select"))) return true;
      if (el.scrollWidth > el.clientWidth + 1) {
        const ox = getComputedStyle(el).overflowX;
        if (ox === "auto" || ox === "scroll") {
          if (dir === 1 && el.scrollLeft > 0) return true;
          if (dir === -1 && el.scrollLeft + el.clientWidth < el.scrollWidth - 1) return true;
        }
      }
    }
  }
  return false;
}

/**
 * The attachment that reads a `Swipe` off the touches on its element. A touch
 * that turns out to be going down is left to scroll; one going sideways that
 * `can` take stops the page scrolling and follows the finger to the end.
 */
export function swipe(s: Swipe) {
  return (node: HTMLElement) => {
    let from: Point | null = null;
    let locked = false;
    /** The last few positions, for the speed it's let go at. */
    let trail: { x: number; t: number }[] = [];

    const reset = () => {
      from = null;
      locked = false;
      trail = [];
    };

    const start = (e: TouchEvent) => {
      reset();
      if (had.has(e) || e.touches.length !== 1) return;
      const t = e.touches[0];
      from = { x: t.clientX, y: t.clientY };
      trail = [{ x: t.clientX, t: e.timeStamp }];
      if (s.holds?.()) had.add(e);
    };

    const move = (e: TouchEvent) => {
      if (!from) return;
      if (e.touches.length !== 1 || (!locked && had.has(e))) return reset();
      const t = e.touches[0];
      const at = { x: t.clientX, y: t.clientY };
      if (!locked) {
        const axis = axisOf(from, at);
        if (!axis) return;
        const dir: Dir = at.x > from.x ? 1 : -1;
        if (axis === "y" || wantedInside(e.target, node, dir, from) || !s.can(dir, from)) {
          // Not ours. A held screen keeps it from the swipes around it too.
          if (s.holds?.()) had.add(e);
          return reset();
        }
        locked = true;
        // Measured from here on, so the screen doesn't jump the slop.
        from = { x: at.x, y: from.y };
      }
      had.add(e);
      if (e.cancelable) e.preventDefault();
      trail.push({ x: at.x, t: e.timeStamp });
      while (trail.length > 2 && e.timeStamp - trail[0].t > 100) trail.shift();
      s.move(at.x - from.x);
    };

    const end = (e: TouchEvent) => {
      if (!from || !locked) return reset();
      const t = e.changedTouches[0];
      const x = t ? t.clientX : trail[trail.length - 1].x;
      const first = trail[0];
      const dt = e.timeStamp - first.t;
      const velocity = dt > 0 ? (x - first.x) / dt : 0;
      const dx = x - from.x;
      reset();
      s.end(dx, velocity);
    };

    const cancel = () => {
      if (from && locked) s.end(0, 0);
      reset();
    };

    node.addEventListener("touchstart", start, { passive: true });
    // Not passive: once it's a swipe, the page mustn't scroll under it.
    node.addEventListener("touchmove", move, { passive: false });
    node.addEventListener("touchend", end);
    node.addEventListener("touchcancel", cancel);
    return () => {
      node.removeEventListener("touchstart", start);
      node.removeEventListener("touchmove", move);
      node.removeEventListener("touchend", end);
      node.removeEventListener("touchcancel", cancel);
    };
  };
}
