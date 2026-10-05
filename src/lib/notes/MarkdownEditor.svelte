<script lang="ts">
  /**
   * Where a note is written: Markdown as typed, coloured as it goes (see
   * `syntax.ts`). A real textarea sits over the colouring with its own text
   * see-through, so everything typing involves — selection, undo, spell check,
   * input methods, the phone's keyboard — is the platform's, not ours.
   *
   * The two layers share one box: the coloured copy sets the height, and the
   * textarea fills it, never scrolling on its own. The pane around them
   * scrolls instead, so the page grows like a document.
   */
  import { highlight } from "./syntax";
  import { lineStart } from "./notes";

  let {
    text,
    onedit,
    jump = null,
    onjumped,
    placeholder = "",
  }: {
    text: string;
    onedit: (text: string) => void;
    /** A line to put the caret on and bring into view, from 1. */
    jump?: number | null;
    onjumped?: () => void;
    placeholder?: string;
  } = $props();

  let area: HTMLTextAreaElement | undefined = $state();
  let layer: HTMLElement | undefined = $state();

  const coloured = $derived(highlight(text));

  // The textarea's value is set here rather than bound, and only when it
  // differs: a note replaced from disk lands without moving the caret more
  // than it has to, and our own typing never rewrites what was just typed.
  $effect(() => {
    if (!area || area.value === text) return;
    const { selectionStart, selectionEnd } = area;
    const focused = document.activeElement === area;
    area.value = text;
    if (focused) area.setSelectionRange(Math.min(selectionStart, text.length), Math.min(selectionEnd, text.length));
  });

  $effect(() => {
    if (!jump || !area || !layer) return;
    const at = lineStart(text, jump);
    area.focus({ preventScroll: true });
    area.setSelectionRange(at, at);
    const line = layer.querySelectorAll<HTMLElement>(".line")[jump - 1];
    line?.scrollIntoView({ block: "center" });
    onjumped?.();
  });

  export function focus() {
    area?.focus();
  }

  /**
   * Type `insert` over the selection as though the user had: through the
   * editing command where the engine has it, so it is one step of the
   * textarea's own undo.
   */
  function type(insert: string) {
    if (!area) return;
    if (!document.execCommand?.("insertText", false, insert)) {
      area.setRangeText(insert, area.selectionStart, area.selectionEnd, "end");
      onedit(area.value);
    }
  }

  /** A list item's marker, kept for the next line: "- ", "2. ", "- [ ] ", "> ". */
  const ITEM = /^(\s*)([-*+]|(\d{1,9})([.)]))(\s+)(\[[ xX]\]\s+)?/;

  function keydown(e: KeyboardEvent) {
    if (!area || e.isComposing) return;
    const { selectionStart: start, selectionEnd: end, value } = area;
    const from = value.lastIndexOf("\n", start - 1) + 1;
    const line = value.slice(from, value.indexOf("\n", start) < 0 ? value.length : value.indexOf("\n", start));

    // Enter in a list carries the list on; Enter on an empty item ends it.
    if (e.key === "Enter" && !e.shiftKey && !e.altKey && !e.ctrlKey && !e.metaKey && start === end) {
      const quote = /^(\s*(?:>\s?)+)/.exec(line);
      const item = ITEM.exec(line);
      const marker = item?.[0] ?? quote?.[0];
      if (!marker || start - from < marker.length) return;
      e.preventDefault();
      if (line.trim() === marker.trim()) {
        area.setSelectionRange(from, from + line.length);
        type("");
        return;
      }
      let next = marker;
      if (item?.[3]) next = `${item[1]}${Number(item[3]) + 1}${item[4]}${item[5]}${item[6] ? "[ ] " : ""}`;
      else if (item?.[6]) next = `${item[1]}${item[2]}${item[5]}[ ] `;
      type("\n" + next);
      return;
    }

    // Tab and Shift+Tab nest a list item. Anywhere else Tab leaves the
    // editor, as it should for the keyboard to get out.
    if (e.key === "Tab" && !e.ctrlKey && !e.metaKey && !e.altKey && ITEM.test(line)) {
      e.preventDefault();
      const caret = start - from;
      if (e.shiftKey) {
        const strip = /^( {1,2}|\t)/.exec(line)?.[0].length ?? 0;
        if (!strip) return;
        area.setSelectionRange(from, from + strip);
        type("");
        area.setSelectionRange(Math.max(from, start - strip), Math.max(from, end - strip));
      } else {
        area.setSelectionRange(from, from);
        type("  ");
        area.setSelectionRange(from + caret + 2, end + 2);
      }
    }
  }
</script>

<div class="editor">
  <!-- Purely the look of the text: the textarea over it is what's read and typed. -->
  <pre class="layer" bind:this={layer} aria-hidden="true">{@html coloured}</pre>
  <textarea
    bind:this={area}
    class="typing"
    spellcheck="true"
    autocapitalize="sentences"
    {placeholder}
    aria-label="Note"
    oninput={(e) => onedit(e.currentTarget.value)}
    onkeydown={keydown}
  ></textarea>
</div>

<style>
  .editor {
    position: relative;
    min-height: 100%;
  }

  /* Both layers are one block of text, set identically, so they wrap alike. */
  .layer,
  .typing {
    margin: 0;
    padding: 0;
    border: 0;
    font-family: var(--note-font, var(--font-mono));
    font-size: var(--note-size, var(--text-2xl));
    line-height: var(--note-leading, 1.7);
    letter-spacing: 0;
    tab-size: 4;
    white-space: pre-wrap;
    overflow-wrap: break-word;
    word-break: normal;
    font-variant-ligatures: none;
  }

  .layer {
    color: var(--fg);
    pointer-events: none;
    min-height: 100%;
  }

  .typing {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    resize: none;
    overflow: hidden;
    background: transparent;
    color: transparent;
    caret-color: var(--accent);
    outline: none;
  }

  .typing::placeholder {
    color: var(--fg-muted);
    opacity: 0.7;
  }

  .typing::selection {
    background: color-mix(in srgb, var(--accent) 26%, transparent);
    color: transparent;
  }

  /* The marks recede, so the words come first. */
  .layer :global(.mk) {
    color: var(--fg-muted);
    opacity: 0.55;
  }

  .layer :global(.h) {
    font-weight: var(--weight-bold);
  }

  .layer :global(.h1) {
    color: var(--fg);
  }

  .layer :global(.h4),
  .layer :global(.h5),
  .layer :global(.h6) {
    color: var(--fg-muted);
  }

  .layer :global(strong) {
    font-weight: var(--weight-bold);
  }

  .layer :global(em) {
    font-style: italic;
  }

  .layer :global(del),
  .layer :global(.done) {
    text-decoration: line-through;
    text-decoration-color: color-mix(in srgb, var(--fg-muted) 70%, transparent);
    color: var(--fg-muted);
  }

  .layer :global(.code),
  .layer :global(.code-block) {
    color: color-mix(in srgb, var(--accent) 45%, var(--fg-muted));
  }

  .layer :global(.bullet) {
    color: var(--accent);
    opacity: 0.9;
  }

  .layer :global(.task) {
    color: var(--accent);
  }

  .layer :global(.task.done) {
    color: var(--success);
  }

  .layer :global(.quote) {
    color: var(--fg-muted);
    font-style: italic;
  }

  .layer :global(.link) {
    color: var(--accent);
  }

  .layer :global(.url) {
    color: var(--fg-muted);
    text-decoration: underline;
    text-decoration-color: color-mix(in srgb, var(--fg-muted) 40%, transparent);
  }

  .layer :global(.meta) {
    color: var(--fg-muted);
  }
</style>
