/**
 * The swipe in from the left edge that iOS apps take to mean "back". A
 * webview gets no such gesture of its own, so the phone layout reads it off
 * the touches itself.
 */

export type Point = { x: number; y: number };

/** How near the left edge a swipe has to start, as iOS's own does. */
export const EDGE = 24;

/** How far right it has to travel before it counts. */
export const TRAVEL = 64;

/**
 * Whether a touch from `from` to `to` was a back swipe: begun at the edge,
 * and more across than down, so scrolling the transcript never takes the
 * user out of it.
 */
export function isBackSwipe(from: Point, to: Point): boolean {
  const across = to.x - from.x;
  return from.x <= EDGE && across >= TRAVEL && Math.abs(to.y - from.y) < across / 2;
}
