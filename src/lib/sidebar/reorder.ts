/**
 * Dragging a project to a new place in the sidebar. The list re-orders live
 * as the dragged row passes its neighbours; these say where it has got to.
 */

/** `ids` with `id` taken out and put back at `index`. */
export function moveTo(ids: string[], id: string, index: number): string[] {
  const rest = ids.filter((x) => x !== id);
  rest.splice(Math.max(0, Math.min(index, rest.length)), 0, id);
  return rest;
}

/**
 * The slot the dragged row belongs in: the one whose top its own top, at `at`,
 * is nearest. `heights` are the other rows', top to bottom, stacked from `top`
 * as they would be without it — so a row gives way once the dragged one is
 * halfway over it, and the slots don't shift under the pointer as it moves.
 */
export function slotFor(heights: number[], top: number, at: number): number {
  let y = top;
  let slot = 0;
  for (const h of heights) {
    if (at < y + h / 2) break;
    y += h;
    slot++;
  }
  return slot;
}
