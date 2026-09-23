/**
 * Just enough of a shell reader to lay out the commands an agent runs. Claude
 * writes them as one long line — `cd … && git … | grep … | head` — which
 * wraps into an unreadable block. This splits a long line at its top-level
 * `&&`, `||`, `|` and `;`, one step per line, and tags each piece so it can
 * be coloured. It never changes what the command says: joining the tokens'
 * text back up gives the command, only with some spaces turned into breaks.
 *
 * It is a highlighter, not a parser. Anything it doesn't understand is left
 * as plain text, and nothing inside quotes, `$(…)` or a heredoc is split.
 */

export type ShellTokenKind =
  | "cmd" // the program a step runs
  | "flag"
  | "string" // quoted text and heredoc bodies
  | "var" // $VAR, ${…}, $(…), and the name in NAME=value
  | "op" // && || | ; & ( ) and redirections
  | "comment"
  | "text";

export type ShellToken = { kind: ShellTokenKind; text: string };

/** Lines up to this long are left on one line. */
const WRAP_AT = 72;

const KEYWORDS = new Set([
  "if", "then", "else", "elif", "fi", "for", "in", "do", "done",
  "while", "until", "case", "esac", "function", "select", "time", "!",
]);

/** Keywords followed by another command, rather than by a name or a word. */
const LEADS_COMMAND = new Set(["if", "then", "else", "elif", "do", "while", "until", "time", "!"]);

type Raw =
  | ShellToken
  | { kind: "ws"; text: string }
  | { kind: "nl"; text: string }
  | { kind: "sep"; text: string }; // a point a long line may break at

const SEPARATORS = ["&&", "||", "|&", ";;", "|", ";"];

/** Index just past the quote, bracket, or backtick span opening at `i`. */
function skipSpan(s: string, i: number): number {
  const c = s[i];
  if (c === "'") {
    const end = s.indexOf("'", i + 1);
    return end === -1 ? s.length : end + 1;
  }
  if (c === '"' || c === "`") {
    let j = i + 1;
    while (j < s.length && s[j] !== c) {
      if (s[j] === "\\") j += 2;
      else if (c === '"' && s[j] === "$" && (s[j + 1] === "(" || s[j + 1] === "{")) j = skipSpan(s, j + 1);
      else j++;
    }
    return Math.min(j + 1, s.length);
  }
  // ( or { — scan to the partner, stepping over anything quoted.
  const close = c === "(" ? ")" : "}";
  let depth = 0;
  let j = i;
  while (j < s.length) {
    const d = s[j];
    if (d === "\\") { j += 2; continue; }
    if (d === "'" || d === '"' || d === "`") { j = skipSpan(s, j); continue; }
    if (d === c) depth++;
    else if (d === close && --depth === 0) return j + 1;
    j++;
  }
  return s.length;
}

function tokenize(s: string): Raw[] {
  const out: Raw[] = [];
  let i = 0;
  let atCommand = true; // the next word is a step's program
  const heredocs: { delim: string; strip: boolean }[] = [];

  const push = (kind: Raw["kind"], text: string) => {
    if (text) out.push({ kind, text } as Raw);
  };

  while (i < s.length) {
    const c = s[i];

    if (c === "\n") {
      push("nl", "\n");
      i++;
      // Heredoc bodies start on the line after their `<<`; they're verbatim.
      for (const h of heredocs.splice(0)) {
        let body = "";
        while (i < s.length) {
          let end = s.indexOf("\n", i);
          if (end === -1) end = s.length;
          const line = s.slice(i, end);
          const done = (h.strip ? line.replace(/^\t+/, "") : line) === h.delim;
          body += line + (end < s.length ? "\n" : "");
          i = end + 1;
          if (done) break;
        }
        push("string", body.endsWith("\n") ? body.slice(0, -1) : body);
        if (body.endsWith("\n")) push("nl", "\n");
      }
      atCommand = true;
      continue;
    }

    if (c === " " || c === "\t") {
      let j = i;
      while (s[j] === " " || s[j] === "\t") j++;
      push("ws", s.slice(i, j));
      i = j;
      continue;
    }

    if (c === "\\" && s[i + 1] === "\n") {
      push("text", "\\");
      i++;
      continue;
    }

    if (c === "#") {
      let end = s.indexOf("\n", i);
      if (end === -1) end = s.length;
      push("comment", s.slice(i, end));
      i = end;
      continue;
    }

    const sep = SEPARATORS.find((op) => s.startsWith(op, i));
    if (sep) {
      push("sep", sep);
      i += sep.length;
      atCommand = true;
      continue;
    }

    // Heredoc: `<<EOF`, `<<-EOF`, `<<'EOF'`, `<< "EOF"`.
    const here = /^<<(-?)[ \t]*(['"]?)([A-Za-z_][\w-]*)\2/.exec(s.slice(i));
    if (here && !s.startsWith("<<<", i)) {
      heredocs.push({ delim: here[3], strip: here[1] === "-" });
      push("op", here[0]);
      i += here[0].length;
      continue;
    }

    const redirect = /^(\d*(?:&>>|&>|>>|>&|<&|<<<|>\||<>|>|<)(?:\d+-?|-)?)/.exec(s.slice(i));
    if (redirect) {
      push("op", redirect[1]);
      i += redirect[1].length;
      continue;
    }

    if (c === "&" || c === "(" || c === ")" || c === "{" || c === "}") {
      // `{` and `}` only group when they stand alone; `{a,b}` is a word.
      const alone = c !== "{" && c !== "}" ? true : /[\s;]/.test(s[i + 1] ?? " ");
      if (alone) {
        push("op", c);
        i++;
        atCommand = c !== ")" && c !== "}";
        continue;
      }
    }

    // A word: runs of plain text, quotes, and expansions up to a space or operator.
    const start = out.length;
    let wordText = "";
    while (i < s.length && !/[\s;&|<>()]/.test(s[i])) {
      const d = s[i];
      let j: number;
      let kind: ShellTokenKind;
      if (d === "'" || d === '"') {
        j = skipSpan(s, i);
        kind = "string";
      } else if (d === "`") {
        j = skipSpan(s, i);
        kind = "var";
      } else if (d === "$" && (s[i + 1] === "(" || s[i + 1] === "{")) {
        j = skipSpan(s, i + 1);
        kind = "var";
      } else if (d === "$" && s[i + 1] === "'") {
        j = skipSpan(s, i + 1);
        kind = "string";
      } else if (d === "$") {
        const m = /^\$(?:[A-Za-z_]\w*|[0-9@*#?$!-])/.exec(s.slice(i));
        j = i + (m ? m[0].length : 1);
        kind = m ? "var" : "text";
      } else {
        j = i;
        while (j < s.length && !/[\s;&|<>()'"`$]/.test(s[j])) j += s[j] === "\\" ? 2 : 1;
        j = Math.min(j, s.length);
        kind = "text";
      }
      const piece = s.slice(i, j);
      const prev = out[out.length - 1];
      if (out.length > start && prev.kind === kind) prev.text += piece;
      else push(kind, piece);
      wordText += piece;
      i = j;
    }
    if (out.length === start) {
      // Nothing we recognise — keep the character so no text is lost.
      push("text", c);
      i++;
      continue;
    }

    // Now that the whole word is known, say what it is.
    const first = out[start] as ShellToken;
    const assign = /^[A-Za-z_]\w*\+?=/.exec(wordText);
    if (atCommand && assign && first.kind === "text" && first.text.startsWith(assign[0])) {
      // NAME=value before a command; the command is still to come.
      first.text = first.text.slice(assign[0].length);
      out.splice(start, 0, { kind: "var", text: assign[0] });
      if (!first.text) out.splice(start + 1, 1);
    } else if (atCommand) {
      if (first.kind === "text" && out.length === start + 1) first.kind = "cmd";
      atCommand = LEADS_COMMAND.has(wordText);
    } else if (first.kind === "text" && first.text.startsWith("-")) {
      first.kind = "flag";
      // `--name=value`: only the name is the flag.
      const eq = first.text.indexOf("=");
      if (eq !== -1) {
        out.splice(start + 1, 0, { kind: "text", text: first.text.slice(eq + 1) });
        first.text = first.text.slice(0, eq + 1);
        if (!out[start + 1].text) out.splice(start + 1, 1);
      }
    }
  }
  return out;
}

/**
 * Tokens for `command`, with any line over `WRAP_AT` broken at its top-level
 * separators: `&&`, `||` and `|` start an indented continuation line, `;`
 * ends its line. Line breaks come back inside `text` tokens.
 */
export function formatShell(command: string): ShellToken[] {
  const raw = tokenize(command.trim());
  const out: ShellToken[] = [];

  let lineStart = 0;
  for (let i = 0; i <= raw.length; i++) {
    if (i < raw.length && raw[i].kind !== "nl") continue;
    const line = raw.slice(lineStart, i);
    lineStart = i + 1;

    const length = line.reduce((n, t) => n + t.text.length, 0);
    const indent = line[0]?.kind === "ws" ? line[0].text : "";
    const wrap = length > WRAP_AT && line.some((t) => t.kind === "sep");

    for (let j = 0; j < line.length; j++) {
      const t = line[j];
      if (t.kind === "sep") {
        if (wrap && t.text !== ";" && t.text !== ";;") {
          // Break before: drop the space the operator sat after.
          if (out.at(-1)?.kind === "text" && /^[ \t]+$/.test(out.at(-1)!.text)) out.pop();
          out.push({ kind: "text", text: `\n${indent}  ` }, { kind: "op", text: t.text });
        } else if (wrap) {
          out.push({ kind: "op", text: t.text }, { kind: "text", text: `\n${indent}` });
          if (line[j + 1]?.kind === "ws") j++;
        } else {
          out.push({ kind: "op", text: t.text });
        }
      } else if (t.kind === "ws" || t.kind === "nl") {
        out.push({ kind: "text", text: t.text });
      } else if (t.kind === "cmd" && KEYWORDS.has(t.text)) {
        out.push({ kind: "op", text: t.text });
      } else {
        out.push(t);
      }
    }
    if (i < raw.length) out.push({ kind: "text", text: "\n" });
  }
  return out;
}

/**
 * What language `command` prints, when all it prints is code in one: files
 * shown with `cat`, `head`, `tail` or `sed -n`, `git show REV:path`, or a
 * diff from `git diff` or `git show`. Leading `cd`s, `echo`ed dividers
 * between files, stderr sent elsewhere and a trailing `| head` are fine.
 * Anything else — files in two languages, a pipe through `grep` — mixes
 * other text in, so it's "" and the output stays plain. Languages come back
 * as file extensions or fence names, as `highlightLines` takes them.
 */
export function outputLanguage(command: string): string {
  const steps = splitSteps(command);
  if (!steps) return "";
  const kept = steps.filter((s, i) => i > 0 && s.sep === "|" ? !trims(s.words) : true);
  if (kept.some((s) => s.sep === "|")) return "";
  let lead = 0;
  while (lead < kept.length - 1 && kept[lead].words[0] === "cd") lead++;
  const langs = new Set(
    kept
      .slice(lead)
      .filter((s) => s.words[0] !== "echo")
      .map((s) => printedLanguage(s.words)),
  );
  return langs.size === 1 ? [...langs][0] : "";
}

/** Whether a step piped into only cuts the output short: `head -50`, `tail -n 20`. */
function trims([cmd, ...args]: string[]): boolean {
  return (cmd === "head" || cmd === "tail") && args.every((w) => /^-|^\d+$/.test(w));
}

/** The language one step prints, or "" if it isn't one that prints a file. */
function printedLanguage([cmd, ...args]: string[]): string {
  const flags = args.filter((w) => w.startsWith("-"));
  let files = args.filter((w) => !w.startsWith("-"));
  switch (cmd) {
    case "cat":
      break;
    case "head":
    case "tail":
      // `-n 40` puts the count in a word of its own.
      files = files.filter((w) => !/^\+?\d+$/.test(w));
      break;
    case "sed":
      if (!flags.includes("-n") || !/^[\d,$]+p$/.test(files.shift() ?? "")) return "";
      break;
    case "git": {
      const [sub, ...rest] = files;
      if (sub === "diff") return "diff";
      if (sub !== "show") return "";
      files = rest.filter((w) => w.includes(":")).map((w) => w.slice(w.indexOf(":") + 1));
      if (files.length === 0) return "diff";
      break;
    }
    default:
      return "";
  }
  const langs = new Set(files.map(extension));
  return langs.size === 1 ? [...langs][0] : "";
}

function extension(path: string): string {
  return /\.([^./\\]+)$/.exec(path)?.[1] ?? "";
}

/**
 * The command's steps as plain words, quotes taken off, each with the
 * separator before it (a line break counts as `;`). Sending stderr away is
 * dropped, as it doesn't change what's printed; `null` for any other
 * redirection, a subshell, a heredoc or `$(…)`, which this won't guess at.
 */
function splitSteps(command: string): { sep: string; words: string[] }[] | null {
  const steps = [{ sep: "", words: [] as string[] }];
  let word: string | null = null;
  let dropNext = false;
  const endWord = () => {
    if (word !== null && !dropNext) steps.at(-1)!.words.push(word);
    else if (word !== null) dropNext = false;
    word = null;
  };
  for (const t of tokenize(command.trim())) {
    if (t.kind === "op") {
      endWord();
      if (t.text === "2>&1") continue;
      if (t.text !== "2>" && t.text !== "2>>") return null;
      dropNext = true;
    } else if (t.kind === "var" && !/^\$\w+$/.test(t.text)) {
      return null;
    } else if (t.kind === "ws" || t.kind === "comment") {
      endWord();
    } else if (t.kind === "sep" || t.kind === "nl") {
      endWord();
      steps.push({ sep: t.kind === "nl" ? ";" : t.text, words: [] });
    } else {
      const text = t.kind === "string" ? t.text.replace(/^\$?(['"])([^]*)\1$/, "$2") : t.text;
      word = (word ?? "") + text;
    }
  }
  endWord();
  return steps.filter((s) => s.words.length > 0);
}

/**
 * A command's output as a terminal would leave it on screen. Tools like
 * cargo colour their output and redraw a progress bar in place, which comes
 * through as escape codes (`\x1b[1m\x1b[92m`) and `\r`-separated redraws.
 * The codes are dropped, and each line keeps only what its last `\r` left —
 * so a progress bar shows its final state, not every step of it.
 */
export function terminalText(output: string): string {
  return output
    .replace(/\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b\[[0-?]*[ -/]*[@-~]|\x1b[@-Z\\-_]/g, "")
    .split("\n")
    .map((line) => line.replace(/\r+$/, "").split("\r").pop()!)
    .join("\n");
}
