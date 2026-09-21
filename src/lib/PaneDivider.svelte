<script lang="ts">
  /**
   * A draggable seam between two panes.
   *
   * It resizes the element immediately before it, measuring the live layout on
   * each drag rather than being told about it: the pane's own left edge sets
   * the origin, and every sibling after the divider except the last (which
   * flexes to fill) is reserved. That keeps the maths right whichever seam
   * this is, and at any window size.
   */

  let {
    label,
    min,
    minLast,
    onresize,
    onreset,
  }: {
    label: string;
    /** Narrowest the preceding pane may become. */
    min: number;
    /** Width kept free for the final, flexing pane. */
    minLast: number;
    onresize: (width: number) => void;
    onreset: () => void;
  } = $props();

  let el: HTMLDivElement;
  let dragging = $state(false);
  let origin = 0;
  let max = 0;
  /** Current pane width, for screen readers. Refreshed whenever it matters. */
  let value = $state(0);
  let ceiling = $state(0);

  /** Latest pointer position, and the frame booked to act on it. */
  let pointerX = 0;
  let frame = 0;

  /** Measure the pane being sized and how far right it may grow. */
  function measure() {
    const pane = el.previousElementSibling as HTMLElement | null;
    const parent = el.parentElement;
    if (!pane || !parent) return null;

    const rect = pane.getBoundingClientRect();
    const last = parent.lastElementChild;
    let reserved = el.offsetWidth;
    for (let n = el.nextElementSibling; n; n = n.nextElementSibling) {
      if (n !== last) reserved += (n as HTMLElement).offsetWidth;
    }

    return {
      left: rect.left,
      width: rect.width,
      max: Math.max(
        min,
        parent.getBoundingClientRect().right - rect.left - reserved - minLast,
      ),
    };
  }

  function clamp(w: number) {
    return Math.min(max, Math.max(min, w));
  }

  /** Push a width out, keeping the announced value in step. */
  function apply(w: number) {
    const next = clamp(w);
    value = Math.round(next);
    onresize(next);
  }

  /**
   * Act on the newest pointer position, once per frame.
   *
   * Pointer events arrive faster than the screen repaints, and every one of
   * them used to resize the pane. Collapsing them onto a frame means the seam
   * is laid out exactly as often as it is drawn.
   */
  function flush() {
    frame = 0;
    if (!dragging) return;
    apply(pointerX - origin);
  }

  function schedule() {
    if (frame) return;
    frame = requestAnimationFrame(flush);
  }

  function onpointerdown(e: PointerEvent) {
    if (e.button !== 0) return;
    const m = measure();
    if (!m) return;
    origin = m.left;
    max = m.max;
    value = Math.round(m.width);
    ceiling = Math.round(m.max);
    pointerX = e.clientX;
    dragging = true;
    el.setPointerCapture(e.pointerId);
    document.body.classList.add("resizing-panes");
    e.preventDefault();
  }

  function onpointermove(e: PointerEvent) {
    if (!dragging) return;
    pointerX = e.clientX;
    schedule();
  }

  function end() {
    if (!dragging) return;
    dragging = false;
    if (frame) {
      cancelAnimationFrame(frame);
      frame = 0;
    }
    document.body.classList.remove("resizing-panes");
  }

  /**
   * The pointer owns the window for the length of a drag.
   *
   * Capture alone isn't enough: it can be handed back early — a window resize
   * or a focus change during the drag is enough to do it — and when that
   * happened the seam simply stopped following the pointer while still looking
   * like it was being dragged. Listening on the window means the drag survives
   * losing capture, and `lostpointercapture` ends it cleanly rather than
   * leaving the app wedged in the resize cursor.
   */
  $effect(() => {
    if (!dragging) return;

    const move = (e: PointerEvent) => onpointermove(e);
    const stop = () => end();

    // The window may change size mid-drag; the origin and the ceiling the
    // drag was clamped against are both measured from it.
    const remeasure = () => {
      const m = measure();
      if (!m) return;
      origin = m.left;
      max = m.max;
      ceiling = Math.round(m.max);
      // Re-run the drag against the new bounds, so the seam stays under the
      // pointer instead of holding a width the window no longer has room for.
      schedule();
    };

    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", stop);
    window.addEventListener("pointercancel", stop);
    window.addEventListener("resize", remeasure);
    el.addEventListener("lostpointercapture", stop);

    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", stop);
      window.removeEventListener("pointercancel", stop);
      window.removeEventListener("resize", remeasure);
      el.removeEventListener("lostpointercapture", stop);
    };
  });

  function onkeydown(e: KeyboardEvent) {
    const step = e.shiftKey ? 48 : 16;
    let delta = 0;
    if (e.key === "ArrowLeft") delta = -step;
    else if (e.key === "ArrowRight") delta = step;
    else if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      onreset();
      return;
    } else return;

    const m = measure();
    if (!m) return;
    max = m.max;
    ceiling = Math.round(m.max);
    e.preventDefault();
    apply(m.width + delta);
  }

  /** Report an accurate width the moment the separator is focused. */
  function onfocus() {
    const m = measure();
    if (!m) return;
    value = Math.round(m.width);
    ceiling = Math.round(m.max);
  }
</script>

<!-- A focusable separator is the ARIA window-splitter pattern; the rules below
     only know the non-focusable kind. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  bind:this={el}
  class="divider"
  class:dragging
  role="separator"
  aria-orientation="vertical"
  aria-label={label}
  aria-valuenow={value}
  aria-valuemin={min}
  aria-valuemax={ceiling}
  tabindex="0"
  {onpointerdown}
  {onkeydown}
  {onfocus}
  ondblclick={onreset}
></div>

<style>
  .divider {
    flex: 0 0 auto;
    width: 1px;
    /* Invisible at rest — the panes' own backgrounds mark the seam — and only
       drawn once the pointer or focus is on it, so it still reads as a handle
       when you go to grab it. */
    background: transparent;
    position: relative;
    cursor: col-resize;
    /* The seam is a hairline; the grab area around it is not. */
    touch-action: none;
  }

  .divider::after {
    content: "";
    position: absolute;
    inset: 0 -4px;
    z-index: var(--z-sticky);
  }

  .divider:hover,
  .divider:focus-visible,
  .divider.dragging {
    background: var(--accent);
    outline: none;
  }

  .divider:focus-visible {
    box-shadow: 0 0 0 1px var(--accent);
  }

  /* While a drag is in flight the pointer owns the window: no text selection,
     and the resize cursor stays put even over a pane's content. */
  :global(body.resizing-panes) {
    cursor: col-resize;
    user-select: none;
  }
</style>
