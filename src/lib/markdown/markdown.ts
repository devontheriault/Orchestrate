import { marked, type Token, type TokensList } from "marked";

/**
 * Claude writes GitHub-flavoured markdown, and writes it for a chat window:
 * a lone newline inside a paragraph is a break it meant, not soft wrapping
 * for something downstream to re-flow. `breaks` keeps its line layout.
 */
const OPTIONS = { gfm: true, breaks: true };

/**
 * Markdown source to a token tree. We stop at tokens rather than going on to
 * HTML: `Markdown.svelte` walks the tree and renders real elements, so agent
 * output never reaches the webview as markup and can't inject any.
 */
export function lex(src: string): TokensList {
  return marked.lexer(src, OPTIONS);
}

export type { Token };

const NAMED: Record<string, string> = {
  amp: "&",
  lt: "<",
  gt: ">",
  quot: '"',
  apos: "'",
  nbsp: " ",
};

/**
 * Markdown says a character reference in prose stands for the character it
 * names. marked leaves them alone because its own renderer emits HTML, where
 * they already read correctly; we emit text nodes, so we decode them here.
 * Code spans and code blocks are excluded, as the spec requires.
 */
export function decodeEntities(text: string): string {
  if (!text.includes("&")) return text;
  return text.replace(/&(#[0-9]+|#x[0-9a-f]+|[a-z]+);/gi, (whole, body: string) => {
    if (body[0] !== "#") return NAMED[body.toLowerCase()] ?? whole;
    const code =
      body[1] === "x" || body[1] === "X"
        ? parseInt(body.slice(2), 16)
        : parseInt(body.slice(1), 10);
    return Number.isInteger(code) && code > 0 && code <= 0x10ffff
      ? String.fromCodePoint(code)
      : whole;
  });
}

/**
 * Only a scheme that means something outside this app gets to be a link —
 * never `javascript:`, and never a bare path, which in agent output is a file
 * in the Worktree rather than somewhere to navigate. Anything else renders as
 * its own text.
 */
export function safeHref(href: string | null | undefined): string | null {
  const h = (href ?? "").trim();
  return /^(https?:|mailto:)/i.test(h) ? h : null;
}

/** A run of plain text, and where it points if it is a URL. */
export type Linked = { text: string; href?: string };

/** What a URL can't hold. The quote and backtick end one written in prose or code. */
const URL = /\bhttps?:\/\/[^\s<>"'`]+/gi;

/**
 * Plain text — a command's output, a prompt, an inline code span — split so
 * the web addresses in it can be links. Punctuation that ends the sentence
 * around a URL isn't part of it, and nor is a closing bracket it never
 * opened: `(see https://x.dev/a)`.
 */
export function linkify(text: string): Linked[] {
  if (!/https?:\/\//i.test(text)) return [{ text }];
  const out: Linked[] = [];
  let last = 0;
  for (const m of text.matchAll(URL)) {
    const href = trimUrl(m[0]);
    if (!href.includes("://") || href.endsWith("://")) continue;
    if (m.index > last) out.push({ text: text.slice(last, m.index) });
    out.push({ text: href, href });
    last = m.index + href.length;
  }
  if (last < text.length) out.push({ text: text.slice(last) });
  return out;
}

const CLOSERS: Record<string, string> = { ")": "(", "]": "[", "}": "{" };

function trimUrl(url: string): string {
  const count = (c: string) => url.split(c).length - 1;
  for (;;) {
    const end = url.at(-1) ?? "";
    const opener = CLOSERS[end];
    const unbalanced = opener !== undefined && count(opener) < count(end);
    if (/[.,;:!?*_~]/.test(end) || unbalanced) url = url.slice(0, -1);
    else return url;
  }
}
