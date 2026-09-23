import type { TokenStream } from "prismjs";

/**
 * Syntax highlighting for code in agent output, as spans to render — never
 * as markup, so code an agent wrote reaches the webview as text, the same
 * as everything else in the transcript.
 */

/** A run of code and the Prism token types it sits inside, as classes. */
export type Span = { text: string; kind: string };

type Prism = typeof import("prismjs");

/** Prism, once its chunk has arrived. Until then code renders plain. */
let prism = $state.raw<Prism | null>(null);
let requested = false;

/**
 * Fetches Prism the first time code asks to be highlighted; everything on
 * screen re-renders in colour when it lands. If it fails, code stays plain.
 */
function loadHighlighter(): void {
  if (requested) return;
  requested = true;
  // Unless told it's driven by hand, Prism restyles every `language-*`
  // element in the document on load.
  (globalThis as { Prism?: unknown }).Prism = { manual: true };
  import("./prism").then(
    (m) => (prism = m.default),
    (e) => console.error("syntax highlighter failed to load", e),
  );
}

/** Fence names and file extensions Prism doesn't register itself. */
const ALIASES: Record<string, string> = {
  rs: "rust",
  golang: "go",
  zsh: "bash",
  shellscript: "bash",
  mjs: "javascript",
  cjs: "javascript",
  mts: "typescript",
  cts: "typescript",
  jsonc: "json",
  htm: "html",
  patch: "diff",
};

/** The language a file is written in, going by its extension. */
export function languageOf(path: string): string {
  return /\.([^./\\]+)$/.exec(path)?.[1] ?? "";
}

/** Beyond this, a file is left plain rather than turned into a sea of spans. */
const LIMIT = 50_000;

/**
 * Lines of code, each split into spans by what its pieces are — or `null`
 * if they can't be: Prism hasn't loaded yet (this asks for it), the language
 * is one we don't ship, or there's too much code to be worth it.
 *
 * The lines are highlighted as one piece, so a comment or string running
 * over several of them colours as it does in the file. They are often cut
 * from the middle of one — a diff hunk, part of a Read, the text an Edit
 * replaces — and in Svelte or HTML that can leave them inside a `<script>`
 * or `<style>` whose tags are out of view. The grammar only colours script
 * it can see the tags of, so the missing tags are stood in for first.
 */
export function highlightLines(lines: string[], lang: string): Span[][] | null {
  const name = ALIASES[lang.toLowerCase()] ?? lang.toLowerCase();
  const [open, close] = MARKUP.has(name) ? missingTags(lines) : [null, null];
  const all = [open, ...lines, close].filter((l) => l !== null);
  const spans = highlight(all.join("\n"), name);
  if (!spans) return null;

  const out: Span[][] = [[]];
  for (const { text, kind } of spans) {
    text.split("\n").forEach((part, i) => {
      if (i > 0) out.push([]);
      if (part) out[out.length - 1].push({ text: part, kind });
    });
  }
  const first = open === null ? 0 : 1;
  return out.slice(first, first + lines.length);
}

function highlight(code: string, name: string): Span[] | null {
  if (!name || code.length > LIMIT) return null;
  const p = prism;
  if (!p) {
    loadHighlighter();
    return null;
  }
  // `languages` also carries Prism's helper functions; only objects are grammars.
  const grammar = Object.hasOwn(p.languages, name) ? p.languages[name] : undefined;
  if (typeof grammar !== "object") return null;

  // What `Prism.highlight` does, short of turning the tokens into HTML. Some
  // grammars (jsx, markdown) finish their work in these hooks.
  const env: Record<string, any> = { code, grammar, language: name };
  p.hooks.run("before-tokenize", env);
  env.tokens = p.tokenize(env.code, env.grammar);
  p.hooks.run("after-tokenize", env);

  const out: Span[] = [];
  flatten(env.tokens, "", out);
  return out;
}

/** Languages whose files hold script and style inside markup. */
const MARKUP = new Set(["svelte", "html", "markup"]);

type Section = "markup" | "script" | "style";

/**
 * The opening tag a fragment of markup is missing, if it starts inside a
 * `<script>` or `<style>`, and the closing tag, if it ends inside one. The
 * first such tag in view says where the fragment starts — a closing one
 * means it began inside — and the last says where it ends.
 */
function missingTags(lines: string[]): [string | null, string | null] {
  let start: Section | null = null;
  let end: Section | null = null;
  for (const line of lines) {
    for (const m of line.matchAll(/<(\/?)(script|style)\b/g)) {
      const tag = m[2] as Section;
      start ??= m[1] ? tag : "markup";
      end = m[1] ? "markup" : tag;
    }
  }
  start ??= guessSection(lines);
  end ??= start;
  return [start === "markup" ? null : `<${start}>`, end === "markup" ? null : `</${end}>`];
}

/**
 * Where a fragment with no `<script>` or `<style>` tags in it sits, from what
 * it looks like: markup has tags and `{#…}` blocks, a stylesheet has
 * selectors and custom properties, and anything else is script — which,
 * inside a component, it usually is.
 */
function guessSection(lines: string[]): Section {
  if (lines.some((l) => /^\s*(<[a-z!/]|\{[#/:@])/i.test(l))) return "markup";
  if (lines.some((l) => /^\s*([.#&@][\w-]|:global\b)|var\(--/.test(l))) return "style";
  return "script";
}

/**
 * Prism nests tokens — a tag holds its punctuation and attributes, a diff
 * line its prefix. Each span keeps every type it sits inside (as `tok-*`, so
 * no Prism name can collide with one of the app's own classes); the
 * stylesheet decides which one shows.
 */
function flatten(stream: TokenStream, kind: string, out: Span[]): void {
  if (typeof stream === "string") {
    const last = out.at(-1);
    if (last && last.kind === kind) last.text += stream;
    else if (stream) out.push({ text: stream, kind });
    return;
  }
  if (Array.isArray(stream)) {
    for (const s of stream) flatten(s, kind, out);
    return;
  }
  const types = [stream.type, ...[stream.alias ?? []].flat()].map((t) => `tok-${t}`);
  flatten(stream.content, [kind, ...types].join(" ").trim(), out);
}
