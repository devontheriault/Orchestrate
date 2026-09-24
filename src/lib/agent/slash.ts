/**
 * The composer's `/` menu, as logic: when it is up, what it offers for what's
 * been typed, and what picking a command does to the text.
 *
 * A slash command is only a command at the very start of a prompt — that is
 * the only place `claude` reads one — so the menu only ever completes the
 * first word.
 */

import type { SlashCommand } from "$lib/api";

/**
 * What's typed of a command so far — the text after a leading `/`, up to the
 * caret — while the caret is still in that first word. Null when there's no
 * command being typed, which is when the menu stays down.
 */
export function slashQuery(text: string, caret: number): string | null {
  const m = /^\/(\S*)$/.exec(text.slice(0, caret));
  return m ? m[1] : null;
}

/**
 * The commands worth offering for `query`, best first: names that start with
 * it, then aliases that do or names with a word that does — so `tdd` finds
 * `mattpocock-skills:tdd` — then names that merely contain it, then commands
 * whose description mentions it. Alphabetical within each, so the list stays
 * put as the query grows.
 */
export function matchCommands(commands: SlashCommand[], query: string): SlashCommand[] {
  const q = query.toLowerCase();
  const rank = (c: SlashCommand): number => {
    const names = [c.name, ...c.aliases].map((n) => n.toLowerCase());
    if (names[0].startsWith(q)) return 0;
    if (names.some((n) => n.split(/[:\-_]/).some((w) => w.startsWith(q)))) return 1;
    if (names.some((n) => n.includes(q))) return 2;
    if (c.description.toLowerCase().includes(q)) return 3;
    return -1;
  };
  return commands
    .map((c) => ({ c, r: rank(c) }))
    .filter(({ r }) => r >= 0)
    .sort((a, b) => a.r - b.r || a.c.name.localeCompare(b.c.name))
    .map(({ c }) => c);
}

/** Whether `query` already names a command in full, so Enter sends it as typed. */
export function namesCommand(commands: SlashCommand[], query: string): boolean {
  return commands.some((c) => c.name === query || c.aliases.includes(query));
}

/**
 * The prompt with its first word replaced by `/name`, and where the caret goes
 * after it: past a space, ready for the command's arguments. Whatever followed
 * the first word stays.
 */
export function completeCommand(text: string, name: string): { text: string; caret: number } {
  const rest = text.replace(/^\/\S*/, "").replace(/^ /, "");
  const head = `/${name} `;
  return { text: head + rest, caret: head.length };
}

/**
 * The command the prompt starts with, once its name is typed in full — for
 * showing what arguments it takes while they're being typed.
 */
export function typedCommand(commands: SlashCommand[], text: string): SlashCommand | null {
  const m = /^\/(\S+)\s/.exec(text);
  if (!m) return null;
  return commands.find((c) => c.name === m[1] || c.aliases.includes(m[1])) ?? null;
}

/**
 * A command's description without what its name already says: plugin skills
 * lead with `(plugin)`, which the namespaced name has just shown.
 */
export function blurb(c: SlashCommand): string {
  const ns = c.name.includes(":") ? c.name.slice(0, c.name.indexOf(":")) : null;
  return ns && c.description.startsWith(`(${ns}) `)
    ? c.description.slice(ns.length + 3)
    : c.description;
}
