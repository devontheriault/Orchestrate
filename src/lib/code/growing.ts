/**
 * Long runs of lines — a patch, a file that was read, a build log — put on
 * the page a chunk at a time, as the reader scrolls toward them. Every line
 * on the page costs in building it, in each layout after, and in memory,
 * whether anyone scrolls down to it or not.
 */

/**
 * Lines put on the page at a time: about three boxes of code deep, and the
 * next chunk is asked for a box before the reader reaches the end. Every
 * open card in a transcript holds at least one chunk, so this is most of
 * what a Write costs to show and to lay out again: at 200 lines, a turn of a
 * dozen Writes took about 22 ms an event and 9 ms more each time a pane was
 * resized.
 */
export const CHUNK = 80;

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
    let io: IntersectionObserver | undefined;
    // Finding the box reads styles, which forces them to be worked out on
    // the spot while the page is still being built. Waiting for the frame
    // lets every marker that mounted together share one pass.
    const frame = requestAnimationFrame(() => {
      io = new IntersectionObserver(
        (entries) => {
          if (!entries.some((e) => e.isIntersecting)) return;
          io?.disconnect();
          then();
        },
        { root: scrollParent(el), rootMargin: "100%" },
      );
      io.observe(el);
    });
    return () => {
      cancelAnimationFrame(frame);
      io?.disconnect();
    };
  };
}
