/**
 * The colours an agent can be tagged with — `/color` in the composer — and the
 * swatch each is drawn in. The names are Claude Code's own `/color` choices, so
 * the command takes what a Claude Code user already types.
 */

export const TAGS = ["red", "orange", "yellow", "green", "cyan", "blue", "purple", "pink"] as const;

export type Tag = (typeof TAGS)[number];

export function isTag(name: string | null | undefined): name is Tag {
  return !!name && (TAGS as readonly string[]).includes(name);
}

/**
 * The CSS colour for an agent's tag, or null when it has none. A tag this build
 * doesn't know — written by a newer one — reads as untagged.
 */
export function tagColor(name: string | null | undefined): string | null {
  return isTag(name) ? `var(--tag-${name})` : null;
}
