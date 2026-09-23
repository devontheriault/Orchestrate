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
 * Fetches Prism the first time it's wanted; every highlighter on screen
 * re-renders in colour when it lands. If it fails, code stays plain.
 */
export function loadHighlighter(): void {
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
 * `code` split into spans by what each piece is, or `null` if it can't be:
 * Prism hasn't loaded yet, the language is one we don't ship, or the code is
 * too long to be worth it.
 */
export function highlight(code: string, lang: string): Span[] | null {
  const p = prism;
  if (!p || !lang || code.length > LIMIT) return null;
  const name = ALIASES[lang.toLowerCase()] ?? lang.toLowerCase();
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
