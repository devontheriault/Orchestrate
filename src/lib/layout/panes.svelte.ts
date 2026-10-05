/**
 * The user-chosen widths of the sidebars: the project pane, and each other
 * Space's.
 *
 * Until the user drags a divider its width is `null`, meaning "whatever the
 * stylesheet says" — for the project pane, the viewport-relative `clamp()`
 * default in theme.css. Once dragged, the width becomes an explicit pixel value
 * that overrides the default and is remembered across restarts.
 */

import type { SpaceId } from "$lib/spaces/space.svelte";
import { viewport } from "./viewport.svelte";

const STORAGE_KEY = "orchestrate:panes";

/** Below this the project list is drawn as a rail of initials, not a tree. */
export const MIN_PROJECTS = 150;
/**
 * A rail is still draggable, so the floor sits below the collapse point — but
 * not below the rail's own natural width (`--rail`, 3.4rem at the largest root
 * font size). Anything narrower would be a stretch of travel where the pointer
 * moves and the seam can't follow it, which reads as a stuck drag.
 */
export const MIN_PROJECTS_DRAG = 56;
/** The detail pane flexes, but never below this. */
export const MIN_DETAIL = 280;

/**
 * The other Spaces' sidebars (ADR 0017), named for what they hold. Unlike the
 * project pane none of them folds to a rail, so each just stops where its rows
 * would stop being readable.
 */
export const MIN_SIDE = {
  notes: 200,
  calendar: 224,
  folders: 150,
  threads: 240,
} as const;

export type Side = keyof typeof MIN_SIDE;

/** Long enough to outlast a drag, short enough to survive a sudden quit. */
const SAVE_DEBOUNCE = 200;

class Panes {
  projects = $state<number | null>(null);

  /** The width the project pane renders at, or null while on its default. */
  width = $derived(this.fit(viewport.width).projects);

  /** True when the project pane is drawn as a rail of initials. */
  railed = $derived(
    this.width === null ? viewport.railed : this.width < MIN_PROJECTS,
  );

  /**
   * The same width as a CSS length, for anything outside the pane that has to
   * line up with its seam — the title bar's lead segment, for one.
   */
  cssWidth = $derived(
    this.width !== null
      ? `${this.width}px`
      : this.railed
        ? "var(--rail)"
        : "var(--pane-projects)",
  );

  /** True while any pane divider is being dragged. */
  dragging = $state(false);

  /** The width of each other Space's sidebar, or null while on its default. */
  sides = $state<Record<Side, number | null>>({
    notes: null,
    calendar: null,
    folders: null,
    threads: null,
  });

  /**
   * How wide each Space's leading sidebar is drawn right now, as measured, or
   * 0 while it has none showing: what the title bar's lead segment spans to top
   * it. Measured rather than worked out, because a sidebar gives up width when
   * the window narrows. Agents isn't here: `cssWidth` is right for it from
   * the first frame.
   */
  lead = $state<Partial<Record<SpaceId, number>>>({});

  private timer: ReturnType<typeof setTimeout> | null = null;

  constructor() {
    if (typeof localStorage === "undefined") return;
    try {
      const raw = localStorage.getItem(STORAGE_KEY);
      if (raw) {
        const v = JSON.parse(raw) as Partial<Record<"projects" | Side, number | null>>;
        this.projects =
          typeof v.projects === "number"
            ? Math.max(MIN_PROJECTS_DRAG, v.projects)
            : null;
        for (const side of Object.keys(MIN_SIDE) as Side[]) {
          const w = v[side];
          if (typeof w === "number") this.sides[side] = Math.max(MIN_SIDE[side], w);
        }
      }
    } catch {
      // A corrupt entry just means "use the defaults".
    }

    // The debounce below can still be in flight when the window goes away.
    if (typeof window !== "undefined") {
      window.addEventListener("pagehide", () => this.flush());
    }
  }

  private save() {
    if (typeof localStorage === "undefined") return;
    try {
      localStorage.setItem(
        STORAGE_KEY,
        JSON.stringify({ projects: this.projects, ...this.sides }),
      );
    } catch {
      // Storage being unavailable shouldn't break resizing.
    }
  }

  /**
   * Write the width out, but not on the drag's critical path.
   *
   * `localStorage` is synchronous, and a drag sets the width once per pointer
   * event. Saving inline meant a blocking write between every pair of frames,
   * which is what made the seam stutter and lag behind the cursor.
   */
  private queueSave() {
    if (this.timer) clearTimeout(this.timer);
    this.timer = setTimeout(() => {
      this.timer = null;
      this.save();
    }, SAVE_DEBOUNCE);
  }

  /** Commit a pending write immediately. */
  flush() {
    if (!this.timer) return;
    clearTimeout(this.timer);
    this.timer = null;
    this.save();
  }

  /**
   * `--pane-projects` and `--rail` for the project pane to set on itself, or
   * undefined to leave it on the stylesheet's defaults.
   *
   * A hand-sized pane keeps its width once it collapses, so the seam carries on
   * tracking the pointer through the rail instead of snapping to the stylesheet
   * rail and sitting there for the rest of the drag. An untouched pane still
   * gets the rail the stylesheet picked.
   *
   * On the pane rather than `<html>`: a custom property changing on the root
   * restyles every element in every open Space, and a drag changes it every
   * frame. With a long transcript open that alone took the drag from 24 ms a
   * frame to 57 ms.
   */
  vars = $derived(this.width === null ? undefined : `${this.width}px`);

  /**
   * Take `--pane-projects` and `--rail` back off `<html>`, where the pre-paint
   * script in `src/app.html` puts the stored width for the page's first frame.
   * From here the pane carries its own (`vars`), and a stale root value would
   * otherwise stand in for the stylesheet's default once the user resets it.
   * Called once, from the page.
   */
  start() {
    $effect(() => {
      const root = document.documentElement.style;
      root.removeProperty("--pane-projects");
      root.removeProperty("--rail");
    });
  }

  /**
   * An attachment for a pane that holds a long run of `items`: while a
   * divider is dragged, the ones out of sight keep the size they had when it
   * began, rather than being laid out again at every width the drag passes
   * through. A dozen open Writes in a transcript are thousands of wrapped
   * lines, and re-wrapping them cost about 15 ms a frame.
   *
   * Only during a drag, not always: an item laid out for the first time as
   * it scrolls in would change height under the reader, and WebKit doesn't
   * anchor the scroll to hide it. And each size is measured and pinned here,
   * not left to `contain-intrinsic-size: auto`, which Chromium only
   * remembers for an item that was already skipping its contents: there,
   * every item collapsed to its placeholder and the pane lost its scroll.
   */
  sitOut(items: string) {
    return (box: HTMLElement) => {
      if (!this.dragging) return;
      const els = [...box.querySelectorAll<HTMLElement>(items)];
      // Every read before any write, so this lays the pane out once.
      const sizes = els.map(contentBox);
      els.forEach((el, i) => {
        el.style.containIntrinsicSize = `${sizes[i].width}px ${sizes[i].height}px`;
        el.style.contentVisibility = "auto";
        // In a flex column, Chromium would otherwise shrink a skipped item
        // past the size pinned on it, down to its padding.
        el.style.flexShrink = "0";
      });
      return () => {
        for (const el of els) {
          el.style.removeProperty("contain-intrinsic-size");
          el.style.removeProperty("content-visibility");
          el.style.removeProperty("flex-shrink");
        }
      };
    };
  }

  setProjects(w: number | null) {
    if (this.projects === w) return;
    this.projects = w;
    this.queueSave();
  }

  setSide(side: Side, w: number | null) {
    if (this.sides[side] === w) return;
    this.sides[side] = w;
    this.queueSave();
  }

  /**
   * A sidebar's width as an inline `flex-basis`, or undefined to leave it on
   * the stylesheet's default.
   */
  basis(side: Side) {
    const w = this.sides[side];
    return w === null ? undefined : `${w}px`;
  }

  /**
   * Give back the width the project pane should render at for a window this
   * wide, shrinking the user's choice rather than letting it squeeze the detail
   * pane out of the window.
   */
  fit(windowWidth: number) {
    let projects = this.projects;

    // A pane still on its default takes a share of the window, so reserve the
    // same share the stylesheet would.
    const p = projects ?? clamp(windowWidth * 0.2, 176, 320);
    const over = p + MIN_DETAIL - windowWidth;

    if (over > 0 && projects !== null) {
      projects = Math.max(MIN_PROJECTS_DRAG, p - over);
    }

    return { projects };
  }
}

/** The size of `el` inside its padding and border, which is what `contain-intrinsic-size` gives. */
function contentBox(el: HTMLElement) {
  const r = el.getBoundingClientRect();
  const s = getComputedStyle(el);
  const px = (...v: string[]) => v.reduce((sum, x) => sum + (parseFloat(x) || 0), 0);
  return {
    width: r.width - px(s.paddingLeft, s.paddingRight, s.borderLeftWidth, s.borderRightWidth),
    height: r.height - px(s.paddingTop, s.paddingBottom, s.borderTopWidth, s.borderBottomWidth),
  };
}

function clamp(v: number, lo: number, hi: number) {
  return Math.min(hi, Math.max(lo, v));
}

export const panes = new Panes();
