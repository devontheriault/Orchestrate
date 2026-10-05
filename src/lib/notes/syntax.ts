/**
 * The colouring under the note editor's text. The editor is a plain textarea,
 * so typing, selection, undo and the OS's input methods are the platform's own;
 * its text is see-through, and this draws the same text beneath it with the
 * Markdown picked out: headings bold, emphasis set, and the marks themselves
 * (`#`, `**`, `>`) quietened so the words read first, as in iA Writer.
 *
 * Every character of the source appears exactly once, in order, in the
 * output, and nothing changes its width — the font is monospaced and bold and
 * italic keep its advance — so the two layers wrap identically and the caret
 * sits on the letter it is drawn over.
 *
 * The result is HTML, built from escaped text and our own spans only.
 */

const ESCAPES: Record<string, string> = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" };

function esc(s: string): string {
  return s.replace(/[&<>"]/g, (c) => ESCAPES[c]);
}

function span(cls: string, s: string): string {
  return s ? `<span class="${cls}">${esc(s)}</span>` : "";
}

/**
 * Inline Markdown in one line of prose: code spans, links, bold, italic,
 * strikethrough. Nested emphasis is left plain inside its outer span, which
 * is enough to read by.
 */
const INLINE =
  /(`+)(.+?)\1|(!?\[)([^\]\n]*)(\]\()([^)\s]*(?:\s+"[^"]*")?)(\))|(\*\*|__)(\S(?:.*?\S)?)\8|(\*|_)(\S(?:.*?\S)?)\10(?![*_\w])|(~~)(\S(?:.*?\S)?)\12|(https?:\/\/[^\s<>()]+)/g;

function inline(line: string): string {
  let out = "";
  let last = 0;
  for (const m of line.matchAll(INLINE)) {
    out += esc(line.slice(last, m.index));
    last = m.index + m[0].length;
    if (m[1] !== undefined) {
      out += `<span class="code">${span("mk", m[1])}${esc(m[2])}${span("mk", m[1])}</span>`;
    } else if (m[3] !== undefined) {
      out +=
        span("mk", m[3]) +
        span("link", m[4]) +
        span("mk", m[5]) +
        span("url", m[6]) +
        span("mk", m[7]);
    } else if (m[8] !== undefined) {
      out += span("mk", m[8]) + `<strong>${esc(m[9])}</strong>` + span("mk", m[8]);
    } else if (m[10] !== undefined) {
      out += span("mk", m[10]) + `<em>${esc(m[11])}</em>` + span("mk", m[10]);
    } else if (m[12] !== undefined) {
      out += span("mk", m[12]) + `<del>${esc(m[13])}</del>` + span("mk", m[12]);
    } else {
      out += span("url", m[14]);
    }
  }
  return out + esc(line.slice(last));
}

/** The editor's layer of colour for `src`, as HTML. */
export function highlight(src: string): string {
  const lines = src.split("\n");
  const out: string[] = [];
  /** The fence that opened the code block we're in: its character and length. */
  let fence: string | null = null;
  let frontMatter = lines[0] === "---";

  for (const [i, line] of lines.entries()) {
    if (frontMatter) {
      out.push(span("meta", line));
      if (i > 0 && (line === "---" || line === "...")) frontMatter = false;
      continue;
    }

    const fenceMark = /^\s{0,3}(`{3,}|~{3,})/.exec(line)?.[1];
    if (fence !== null) {
      const closes =
        fenceMark !== undefined &&
        fenceMark[0] === fence[0] &&
        fenceMark.length >= fence.length &&
        line.trim() === fenceMark;
      out.push(span(closes ? "mk" : "code-block", line));
      if (closes) fence = null;
      continue;
    }
    if (fenceMark !== undefined) {
      fence = fenceMark;
      out.push(span("mk", line));
      continue;
    }

    const h = /^(\s{0,3}#{1,6})(\s+)(.*)$/.exec(line) ?? /^(\s{0,3}#{1,6})()()$/.exec(line);
    if (h) {
      const level = h[1].trim().length;
      out.push(`${span("mk", h[1] + h[2])}<span class="h h${level}">${inline(h[3])}</span>`);
      continue;
    }

    if (/^\s{0,3}([-*_])(\s*\1){2,}\s*$/.test(line)) {
      out.push(span("mk", line));
      continue;
    }

    const quote = /^(\s*(?:>\s?)+)(.*)$/.exec(line);
    if (quote) {
      out.push(`${span("mk", quote[1])}<span class="quote">${inline(quote[2])}</span>`);
      continue;
    }

    const item = /^(\s*)([-*+]|\d{1,9}[.)])(\s+)(\[[ xX]\]\s)?(.*)$/.exec(line);
    if (item) {
      const done = item[4] !== undefined && /x/i.test(item[4]);
      out.push(
        esc(item[1]) +
          span("mk bullet", item[2]) +
          esc(item[3]) +
          (item[4] ? span(done ? "task done" : "task", item[4]) : "") +
          (done ? `<span class="done">${inline(item[5])}</span>` : inline(item[5])),
      );
      continue;
    }

    out.push(inline(line));
  }
  // A textarea ending in a newline shows an empty last line; a <pre> doesn't,
  // unless something follows it.
  return out.map((l) => `<span class="line">${l}</span>`).join("\n") + "\n ";
}
