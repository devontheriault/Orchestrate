/**
 * The Notes Space's pure logic: folders, titles, dates, and which rows of a
 * long list are on screen. Kept apart from the store and the components so it
 * reads, and can be checked, without a Host.
 */

import type { NoteSummary } from "$lib/api";

/** The folder a note sits in, relative to the notes folder; "" at the top. */
export function folderOf(path: string): string {
  const at = path.lastIndexOf("/");
  return at < 0 ? "" : path.slice(0, at);
}

/** The file name without its extension. */
export function nameOf(path: string): string {
  return path.slice(path.lastIndexOf("/") + 1).replace(/\.(md|markdown)$/i, "");
}

/** Whether `path` is in `folder` or below it. "" holds everything. */
export function inFolder(path: string, folder: string): boolean {
  return folder === "" || path.startsWith(folder + "/");
}

/** One row of the folder tree. */
export type FolderRow = {
  path: string;
  name: string;
  depth: number;
  /** Whether it has folders under it, so it can fold. */
  parent: boolean;
  /** Notes in it and below it. */
  count: number;
};

/**
 * The folders as an indented tree, with the folded ones' insides left out.
 * `folders` comes sorted from the Host, which puts a parent right before its
 * children.
 */
export function folderRows(
  folders: string[],
  notes: NoteSummary[],
  folded: Record<string, boolean>,
): FolderRow[] {
  const counts = new Map<string, number>();
  for (const n of notes) {
    let dir = folderOf(n.path);
    while (dir) {
      counts.set(dir, (counts.get(dir) ?? 0) + 1);
      dir = folderOf(dir);
    }
  }
  const rows: FolderRow[] = [];
  for (const [i, path] of folders.entries()) {
    if (ancestors(path).some((a) => folded[a])) continue;
    rows.push({
      path,
      name: path.slice(path.lastIndexOf("/") + 1),
      depth: path.split("/").length - 1,
      parent: folders[i + 1]?.startsWith(path + "/") ?? false,
      count: counts.get(path) ?? 0,
    });
  }
  return rows;
}

function ancestors(path: string): string[] {
  const out: string[] = [];
  for (let dir = folderOf(path); dir; dir = folderOf(dir)) out.push(dir);
  return out;
}

/** An ATX heading's text, as the Host reads one (`notes/text.rs`). */
function heading(line: string): string | null {
  const m = /^\s*#{1,6}(?:[ \t]+(.*?))?[ \t#]*$/.exec(line);
  const text = m?.[1]?.replace(/[ \t]+#+$/, "").trim();
  return text ? text : null;
}

/**
 * The title the Host would give this text: the first line when it is a
 * heading, else the first `# ` heading. Null when it has neither, and the
 * file name stands in.
 */
export function titleOf(text: string): string | null {
  const body = text.replace(/^---\r?\n[\s\S]*?\r?\n(?:---|\.\.\.)\r?\n/, "");
  const lines = body.split("\n");
  const first = lines.find((l) => l.trim() !== "");
  const fromFirst = first !== undefined ? heading(first) : null;
  if (fromFirst) return fromFirst;
  for (const l of lines) {
    if (/^\s*# /.test(l)) {
      const h = heading(l);
      if (h) return h;
    }
  }
  return null;
}

/** A note still named as it was made, waiting for a title to name it by. */
export function isUntitled(path: string): boolean {
  return /^Untitled(?: \d+)?\.md$/.test(path.slice(path.lastIndexOf("/") + 1));
}

/**
 * Where an untitled note should move once it has a title: beside itself,
 * named after it. Null if it has no title yet, or no name it could take.
 * The Host cleans the name further, and refuses to land on another file.
 */
export function namedPath(path: string, text: string): string | null {
  const title = titleOf(text);
  if (!title) return null;
  const stem = title
    .replace(/[/\\:*?"<>|\u0000-\u001f]/g, " ")
    .replace(/\s+/g, " ")
    .replace(/^[.\s]+|[.\s]+$/g, "")
    .slice(0, 80)
    .trim();
  if (!stem) return null;
  const dir = folderOf(path);
  return `${dir ? dir + "/" : ""}${stem}.md`;
}

const DAY = 86_400_000;

/**
 * When a note last changed, as briefly as reads clearly: "now", "12m", a
 * time today, "Yesterday", a date this year, or a full date.
 */
export function when(ms: number, now = Date.now()): string {
  const ago = now - ms;
  if (ago < 60_000) return "now";
  if (ago < 3_600_000) return `${Math.floor(ago / 60_000)}m`;
  const then = new Date(ms);
  const today = new Date(now);
  today.setHours(0, 0, 0, 0);
  if (ms >= today.getTime()) {
    return then.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
  }
  if (ms >= today.getTime() - DAY) return "Yesterday";
  const sameYear = then.getFullYear() === today.getFullYear();
  return then.toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
    ...(sameYear ? {} : { year: "numeric" }),
  });
}

/** A run of text, and whether it matched the search. */
export type Marked = { text: string; hit: boolean };

/** `text` split so the words of `query` can be marked where they appear. */
export function mark(text: string, query: string): Marked[] {
  const terms = query
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean)
    .map((t) => t.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"));
  if (!terms.length || !text) return [{ text, hit: false }];
  const out: Marked[] = [];
  let last = 0;
  for (const m of text.matchAll(new RegExp(terms.join("|"), "gi"))) {
    if (m.index > last) out.push({ text: text.slice(last, m.index), hit: false });
    out.push({ text: m[0], hit: true });
    last = m.index + m[0].length;
  }
  if (last < text.length) out.push({ text: text.slice(last), hit: false });
  return out;
}

/**
 * The rows of a list of `count` rows, each `row` pixels tall, that a box
 * `height` pixels tall scrolled to `top` shows — with `spare` more either
 * side, so a quick scroll doesn't flash blank rows.
 */
export function onScreen(
  top: number,
  height: number,
  row: number,
  count: number,
  spare = 8,
): { start: number; end: number } {
  if (row <= 0) return { start: 0, end: Math.min(count, 50) };
  const start = Math.max(0, Math.floor(top / row) - spare);
  const end = Math.min(count, Math.ceil((top + height) / row) + spare);
  return { start, end };
}

/** Where line `n` (from 1) starts in `text`, as a character offset. */
export function lineStart(text: string, n: number): number {
  let at = 0;
  for (let i = 1; i < n; i++) {
    const next = text.indexOf("\n", at);
    if (next < 0) return text.length;
    at = next + 1;
  }
  return at;
}
