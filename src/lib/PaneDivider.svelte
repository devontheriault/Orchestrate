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

  function onpointerdown(e: PointerEvent) {
    if (e.button !== 0) return;
    const m = measure();
    if (!m) return;
    origin = m.left;
    max = m.max;
    value = Math.round(m.width);
    ceiling = Math.round(m.max);
    dragging = true;
    el.setPointerCapture(e.pointerId);
    document.body.classList.add("resizing-panes");
    e.preventDefault();
  }

  function onpointermove(e: PointerEvent) {
    if (!dragging) return;
    const w = clamp(e.clientX - origin);
    value = Math.round(w);
    onresize(w);
  }

  function end(e: PointerEvent) {
    if (!dragging) return;
    dragging = false;
    if (el.hasPointerCapture(e.pointerId)) el.releasePointerCapture(e.pointerId);
    document.body.classList.remove("resizing-panes");
  }

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
    const w = clamp(m.width + delta);
    value = Math.round(w);
    onresize(w);
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
  {onpointermove}
  onpointerup={end}
  onpointercancel={end}
  {onkeydown}
  {onfocus}
  ondblclick={onreset}
></div>

<style>
  .divider {
    flex: 0 0 auto;
    width: 1px;
    background: var(--border);
    position: relative;
    cursor: col-resize;
    /* The seam is a hairline; the grab area around it is not. */
    touch-action: none;
  }

  .divider::after {
    content: "";
    position: absolute;
    inset: 0 -4px;
    z-index: 5;
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
