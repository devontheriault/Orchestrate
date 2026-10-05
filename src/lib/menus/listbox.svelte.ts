/**
 * A drop-down list of choices, as every single-level picker runs it: a trigger
 * that opens it, a list the keyboard walks, and `menu.ts` to place it and to
 * dismiss it. The picker keeps its own rows, and what a pick does.
 *
 * Made while a component is being set up, since it starts effects of its own.
 */

import { dismissOnMove, opensMenu, stepActive, type Placement } from "./menu";

export class Listbox {
  open = $state(false);
  /** Index the keyboard is on while the list is up. */
  active = $state(0);
  /**
   * Whether the last move came from the keyboard. Only then is it right to
   * scroll the list — doing it on hover fires a scroll the dismisser sees.
   */
  keyNav = $state(false);
  placement = $state<Placement | null>(null);
  trigger = $state<HTMLElement>();
  list = $state<HTMLElement>();

  #count: () => number;
  #place: (trigger: HTMLElement) => Placement;
  #pick: (index: number) => void;

  constructor({
    count,
    place,
    pick,
  }: {
    /** How many rows the list has. */
    count: () => number;
    /** Where the list goes, against its trigger. */
    place: (trigger: HTMLElement) => Placement;
    /** The row at this index was picked, by click or key. */
    pick: (index: number) => void;
  }) {
    this.#count = count;
    this.#place = place;
    this.#pick = pick;

    // The list is pinned to the trigger's position, so anything that moves it
    // dismisses it rather than leaving it stranded mid-air.
    $effect(() => {
      if (!this.open) return;
      return dismissOnMove(() => [this.list, this.trigger], () => this.close(false));
    });

    // Keys land on the list, per the ARIA listbox pattern.
    $effect(() => {
      if (this.open) this.list?.focus();
    });

    // A list longer than its box opens on the current choice, not at the top.
    $effect(() => {
      if (!this.open || !this.keyNav) return;
      void this.active;
      this.list
        ?.querySelector<HTMLElement>('[data-active="true"]')
        ?.scrollIntoView({ block: "nearest" });
    });
  }

  /** Open the list with the keyboard on row `at`. */
  show(at: number) {
    if (!this.trigger || this.#count() === 0) return;
    this.active = Math.max(0, at);
    this.keyNav = true;
    this.placement = this.#place(this.trigger);
    this.open = true;
  }

  close(refocus = true) {
    if (!this.open) return;
    this.open = false;
    if (refocus) this.trigger?.focus();
  }

  /** What a click on the trigger does. */
  toggle(at: number) {
    if (this.open) this.close();
    else this.show(at);
  }

  /** The pointer is over row `i`. */
  hover(i: number) {
    this.keyNav = false;
    this.active = i;
  }

  /** A key on the closed list's trigger: `at` is the row it opens on. */
  triggerKey(e: KeyboardEvent, at: number) {
    if (!opensMenu(e.key)) return;
    e.preventDefault();
    this.show(at);
  }

  /** A key on the open list. */
  listKey(e: KeyboardEvent) {
    this.keyNav = true;
    const next = stepActive(e.key, this.active, this.#count());
    if (next !== null) {
      e.preventDefault();
      this.active = next;
      return;
    }
    switch (e.key) {
      case "Escape":
        // The list may sit in a popover that Escape closes too.
        e.preventDefault();
        e.stopPropagation();
        this.close();
        break;
      case "Tab":
        this.close(false);
        break;
      case "Enter":
      case " ":
        e.preventDefault();
        this.#pick(this.active);
        break;
    }
  }
}
