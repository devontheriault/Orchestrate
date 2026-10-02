/**
 * Long runs of lines — a patch, a file that was read, a build log — put on
 * the page a chunk at a time, as the reader scrolls toward them. Every line
 * on the page costs in building it, in each layout after, and in memory,
 * whether anyone scrolls down to it or not.
 */

/** Lines put on the page at a time: several boxes of code deep. */
export const CHUNK = 200;

/** The nearest ancestor of `el` that scrolls up and down, or null for the page. */
function scrollParent(el: Element): Element | null {
  for (let p = el.parentElement; p; p = p.parentElement) {
    const { overflowY } = getComputedStyle(p);
    if (overflowY === "auto" || overflowY === "scroll") return p;
  }
  return null;
}

/**
 * An attachment that calls `then` once `el` comes within a box's height of
 * the box it scrolls in, then stops watching.
 */
export function whenNear(then: () => void) {
  return (el: Element) => {
    const io = new IntersectionObserver(
      (entries) => {
        if (!entries.some((e) => e.isIntersecting)) return;
        io.disconnect();
        then();
      },
      { root: scrollParent(el), rootMargin: "100%" },
    );
    io.observe(el);
    return () => io.disconnect();
  };
}
