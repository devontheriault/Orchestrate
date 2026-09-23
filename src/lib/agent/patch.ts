/**
 * A worktree's patch, as the Diff tab draws it: split into files, then
 * coloured a hunk at a time.
 */

import { highlightLines, type Span } from "$lib/code/highlight.svelte";

/** Cap on rendered patch lines, so a huge diff can't stall the webview. */
const MAX_LINES = 6000;

export type PatchLine = { text: string; kind: "add" | "del" | "hunk" | "ctx" };
export type PatchFile = { path: string; lines: PatchLine[] };

/**
 * Split the raw patch into per-file sections, dropping the git metadata
 * lines (index/---/+++/mode) that the file header already conveys. Stops
 * keeping lines after `MAX_LINES`, and says so with `clipped`.
 */
export function parsePatch(patch: string): { files: PatchFile[]; clipped: boolean } {
  if (!patch) return { files: [], clipped: false };

  const files: PatchFile[] = [];
  let budget = MAX_LINES;
  let clipped = false;

  for (const raw of patch.split("\n")) {
    if (raw.startsWith("diff --git ")) {
      const m = raw.match(/ b\/(.*)$/);
      files.push({ path: m ? m[1] : raw.slice("diff --git ".length), lines: [] });
      continue;
    }
    const current = files[files.length - 1];
    if (!current) continue;
    if (
      /^(index |--- |\+\+\+ |old mode |new mode |new file mode |deleted file mode |similarity index |rename (from|to) )/.test(
        raw,
      )
    ) {
      continue;
    }
    if (budget <= 0) {
      clipped = true;
      continue;
    }
    budget--;
    const kind: PatchLine["kind"] = raw.startsWith("@@")
      ? "hunk"
      : raw.startsWith("+")
        ? "add"
        : raw.startsWith("-")
          ? "del"
          : "ctx";
    current.lines.push({ text: raw, kind });
  }
  return { files, clipped };
}

/**
 * Each patch line's code as coloured spans, or null where a line stays
 * plain. A hunk is highlighted as the two pieces of the file it shows —
 * before (context and deletions) and after (context and additions) — so a
 * comment or string running over several lines colours as it does in the
 * file, as far as the hunk reaches.
 */
export function highlightPatch(lines: PatchLine[], lang: string): (Span[] | null)[] {
  const out: (Span[] | null)[] = lines.map(() => null);
  let start = 0;
  for (let i = 0; i <= lines.length; i++) {
    if (i < lines.length && lines[i].kind !== "hunk") continue;
    const before: number[] = [];
    const after: number[] = [];
    for (let j = start; j < i; j++) {
      // Skips git's "\ No newline at end of file" and the patch's last, empty line.
      if (lines[j].text === "" || lines[j].text.startsWith("\\")) continue;
      if (lines[j].kind !== "add") before.push(j);
      if (lines[j].kind !== "del") after.push(j);
    }
    for (const side of [before, after]) {
      const spans = highlightLines(side.map((j) => lines[j].text.slice(1)), lang);
      side.forEach((j, k) => (out[j] ??= spans?.[k] ?? null));
    }
    start = i + 1;
  }
  return out;
}
