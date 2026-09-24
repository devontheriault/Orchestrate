/**
 * What every drop-down menu in the app shares: where it sits, what dismisses
 * it, and how the keyboard walks it. The menus themselves — the model, mode
 * and branch pickers, the settings menu and the composer's `/` menu — keep
 * their own rows and their own rules about what a pick does.
 *
 * They are ours rather than a native `<select>` because the OS draws its list
 * in the desktop theme's colours, ignores the stylesheet, and has no submenu.
 * They are `position: fixed` so the panes' `overflow: hidden` can't clip them,
 * which is why they are placed by measuring their trigger.
 */

/** Where a fixed menu sits — anchored to what opened it, flipped if needed. */
export type Placement = { left: number; width: number; maxHeight: number; y: string };

/** Gap between a trigger and its menu. */
const GAP = 6;
/** Closest a menu may come to the window's edge. */
const EDGE = 8;

/**
 * Place a menu against `trigger`, below it or above it.
 *
 * With a `maxHeight`, the menu drops down unless only the space above has room
 * for it, and never grows past that height. Without one it is a long list that
 * scrolls in whatever height it gets, so it simply takes the roomier side.
 *
 * `align` is the edge the menu lines up with the trigger on.
 */
export function placeMenu(
  trigger: HTMLElement,
  {
    minWidth,
    maxHeight,
    align,
  }: { minWidth: number; maxHeight?: number; align: "left" | "right" },
): Placement {
  const r = trigger.getBoundingClientRect();
  const below = window.innerHeight - r.bottom - GAP - EDGE;
  const above = r.top - GAP - EDGE;
  const up = maxHeight === undefined ? above >= below : below < Math.min(maxHeight, above);
  const room = up ? above : below;
  const width = Math.max(r.width, minWidth);
  const anchor = align === "right" ? r.right - width : r.left;
  return {
    width,
    left: Math.min(Math.max(EDGE, anchor), window.innerWidth - width - EDGE),
    maxHeight:
      maxHeight === undefined
        ? Math.max(160, Math.round(room))
        : Math.max(120, Math.min(maxHeight, room)),
    y: up
      ? `bottom: ${Math.round(window.innerHeight - r.top + GAP)}px`
      : `top: ${Math.round(r.bottom + GAP)}px`,
  };
}

/** How far a submenu's first row sits below its top edge: the menu's padding and border. */
const SUB_INSET = 5;

/**
 * Hang a submenu off `row`, one of the rows of `menu`: beside the menu on
 * whichever side has the room, and level with the row — growing down from its
 * top, or up from its bottom when the row is in the lower half of the window.
 * It takes all the height that side has and scrolls inside it.
 */
export function placeSubmenu(row: HTMLElement, menu: HTMLElement, width: number): Placement {
  const r = row.getBoundingClientRect();
  const m = menu.getBoundingClientRect();
  // No gap: the pointer has to cross from the row into the submenu, and a gap
  // there is a dead zone that closes it mid-travel.
  const left = m.right + width <= window.innerWidth - EDGE ? m.right : m.left - width;
  const up = r.top + r.bottom > window.innerHeight;
  const top = Math.max(EDGE, r.top - SUB_INSET);
  const bottom = Math.min(window.innerHeight - EDGE, r.bottom + SUB_INSET);
  return {
    width,
    left: Math.max(EDGE, left),
    maxHeight: up ? bottom - EDGE : window.innerHeight - EDGE - top,
    y: up
      ? `bottom: ${Math.round(window.innerHeight - bottom)}px`
      : `top: ${Math.round(top)}px`,
  };
}

/** A placement as the menu's inline style. */
export function menuStyle(p: Placement): string {
  return `left: ${Math.round(p.left)}px; width: ${Math.round(p.width)}px; max-height: ${Math.round(p.maxHeight)}px; ${p.y}`;
}

/**
 * Close a menu when anything would move its trigger out from under it: a press
 * outside it, a resize, or a scroll anywhere but inside it. `inside` names the
 * elements that count as the menu — its trigger, its list, any submenu — and is
 * asked at the moment of each event, since they bind after the menu opens.
 *
 * `scroll: false` is for a menu whose trigger no scroll can move — the
 * composer's `/` menu sits on the input box, while the transcript above it
 * scrolls on its own every time a working agent says something.
 *
 * Returns the cleanup, for the `$effect` that calls it to hand back.
 */
export function dismissOnMove(
  inside: () => (Element | undefined)[],
  close: () => void,
  { scroll = true }: { scroll?: boolean } = {},
): () => void {
  const ours = (t: Node | null) => !!t && inside().some((el) => !!el?.contains(t));
  const onDown = (e: PointerEvent) => {
    if (ours(e.target as Node)) return;
    close();
  };
  const onScroll = (e: Event) => {
    // A menu scrolling inside itself isn't the page moving under it.
    if (!scroll || ours(e.target as Node)) return;
    close();
  };
  const onResize = () => close();
  window.addEventListener("pointerdown", onDown, true);
  window.addEventListener("resize", onResize);
  // Capture: the transcript, the diff and the project list scroll, not the window.
  window.addEventListener("scroll", onScroll, true);
  return () => {
    window.removeEventListener("pointerdown", onDown, true);
    window.removeEventListener("resize", onResize);
    window.removeEventListener("scroll", onScroll, true);
  };
}

/** Whether a key pressed on a closed menu's trigger opens it. */
export function opensMenu(key: string): boolean {
  return key === "ArrowDown" || key === "ArrowUp" || key === "Enter" || key === " ";
}

/**
 * Where a key moves the highlight in a list of `count` rows: the arrows step
 * and wrap, Home and End jump. Null for any other key.
 */
export function stepActive(key: string, active: number, count: number): number | null {
  switch (key) {
    case "ArrowDown":
      return (active + 1) % count;
    case "ArrowUp":
      return (active - 1 + count) % count;
    case "Home":
      return 0;
    case "End":
      return count - 1;
  }
  return null;
}
