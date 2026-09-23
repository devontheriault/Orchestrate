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
