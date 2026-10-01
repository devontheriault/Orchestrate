/**
 * The window's size, as reactive state, plus the layout mode derived from it.
 *
 * Most of the responsive work is plain CSS; this exists for the one thing CSS
 * can't decide on its own — how the project pane gives up width when the
 * window runs out of it, and whether a phone shows one screen at a time.
 */

import { mobile } from "./platform";

/**
 * Below this, the window can't hold a readable tree and a readable detail pane
 * at the same time, so the tree shrinks to a rail of initials. It never leaves:
 * switching projects and agents is the app's main move, and a navigation pane
 * you have to go back to isn't navigation.
 */
const RAIL_AT = 520;

/**
 * Below this, on a phone, the app is one screen at a time: the project list,
 * or the agent opened from it, with a way back. A phone is held for one thing
 * at a time, and a rail of initials beside 300px of transcript serves neither.
 * A tablet is wider, and keeps the panes side by side.
 */
const PHONE_AT = 700;

/**
 * How much of the screen has to be covered before it counts as the on-screen
 * keyboard: more than the bars a phone slides in and out while scrolling.
 */
const KEYBOARD_AT = 120;

class Viewport {
  width = $state(1280);
  height = $state(800);

  private stopFn: (() => void) | null = null;
  private frame = 0;

  /** True once the window is too tight for the tree at a readable width. */
  railed = $derived(this.width < RAIL_AT);

  /** True on a phone-sized screen: one screen at a time (see `PHONE_AT`). */
  phone = $derived(mobile && this.width < PHONE_AT);

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

    // iOS lays the on-screen keyboard over the page rather than shrinking it,
    // then scrolls the whole page up to keep the input in view, taking the
    // header off the top. So on a phone the page is sized to what's visible
    // above the keyboard (`--visible-h`, read by theme.css), and put back at
    // the top whenever iOS has scrolled it.
    const visual = mobile ? window.visualViewport : null;
    // While the keyboard is up it covers the home indicator too, so nothing
    // needs to keep clear of that (`html[data-keyboard]` in theme.css).
    const fitVisible = () => {
      if (!visual) return;
      const root = document.documentElement;
      root.style.setProperty("--visible-h", `${visual.height}px`);
      root.toggleAttribute("data-keyboard", window.innerHeight - visual.height > KEYBOARD_AT);
      if (window.scrollY) window.scrollTo(0, 0);
    };
    fitVisible();
    visual?.addEventListener("resize", fitVisible);
    visual?.addEventListener("scroll", fitVisible);

    this.stopFn = () => {
      window.removeEventListener("resize", schedule);
      visual?.removeEventListener("resize", fitVisible);
      visual?.removeEventListener("scroll", fitVisible);
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
