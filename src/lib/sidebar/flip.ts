/**
 * Gliding the sidebar's rows to where they now sit, rather than letting them
 * jump, when an agent changes heading. Measure before the list re-renders,
 * then play after: each marked element starts where it was and eases into
 * place. The row that moved is a new element under its new heading, so rows
 * are matched by key, not by node.
 */

const DURATION = 280;
const EASING = "cubic-bezier(0.2, 0, 0, 1)";

/** Where each element marked `data-flip="<key>"` inside `root` sits now. */
export function measure(root: HTMLElement): Map<string, DOMRect> {
  const rects = new Map<string, DOMRect>();
  for (const el of root.querySelectorAll<HTMLElement>("[data-flip]")) {
    rects.set(el.dataset.flip!, el.getBoundingClientRect());
  }
  return rects;
}

/**
 * Slides every marked element from its measured spot to where it is now, and
 * fades in the ones that weren't on screen before, such as a heading that has
 * just gained its first agent. Keys in `moved` — the agents that changed
 * heading — also come up from faint, so the eye lands on them.
 */
export function glide(root: HTMLElement, before: Map<string, DOMRect>, moved: Set<string>) {
  if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
  for (const el of root.querySelectorAll<HTMLElement>("[data-flip]")) {
    const key = el.dataset.flip!;
    const was = before.get(key);
    if (!was) {
      el.animate({ opacity: [0, 1] }, { duration: DURATION, easing: EASING });
      continue;
    }
    const dy = was.top - el.getBoundingClientRect().top;
    if (Math.abs(dy) < 1 && !moved.has(key)) continue;
    const from: Keyframe = { transform: `translateY(${dy}px)` };
    const to: Keyframe = { transform: "none" };
    if (moved.has(key)) {
      from.opacity = 0.5;
      to.opacity = 1;
    }
    el.animate([from, to], { duration: DURATION, easing: EASING });
  }
}
