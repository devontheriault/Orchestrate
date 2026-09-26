<script lang="ts">
  import MarkdownBlocks from "./MarkdownBlocks.svelte";
  import { lex } from "./markdown";

  /** Markdown source — what Claude actually writes. */
  let { text }: { text: string } = $props();

  const tokens = $derived(lex(text ?? ""));
</script>

<div class="md"><MarkdownBlocks {tokens} /></div>

<style>
  /* One stylesheet for the whole rendered document: `MarkdownBlocks` and
     `MarkdownInline` render inside this element and carry no styles of their
     own, so the look lives in one place and nesting costs nothing. Links are
     the exception: they show in plain output too, so `Link` styles itself. */
  .md {
    min-width: 0;
  }

  .md > :global(:first-child) {
    margin-top: 0;
  }

  .md > :global(:last-child) {
    margin-bottom: 0;
  }

  .md :global(p) {
    margin: 0 0 0.6em;
    overflow-wrap: anywhere;
  }

  .md :global(h1),
  .md :global(h2),
  .md :global(h3),
  .md :global(h4),
  .md :global(h5),
  .md :global(h6) {
    margin: 1.1em 0 0.45em;
    line-height: var(--leading-tight);
    font-weight: var(--weight-semibold);
    overflow-wrap: anywhere;
  }

  .md :global(h1) { font-size: 1.3em; }
  .md :global(h2) { font-size: 1.15em; }
  .md :global(h3) { font-size: 1.05em; }

  .md :global(h4),
  .md :global(h5),
  .md :global(h6) {
    font-size: 1em;
    color: var(--fg-muted);
  }

  .md :global(ul),
  .md :global(ol) {
    margin: 0 0 0.6em;
    padding-left: 1.45em;
  }

  .md :global(li) {
    margin: 0.12em 0;
    overflow-wrap: anywhere;
  }

  /* A nested list belongs to the line above it, not to the gap after it. */
  .md :global(li > ul),
  .md :global(li > ol) {
    margin-bottom: 0.12em;
  }

  .md :global(li > p) {
    margin-bottom: 0.3em;
  }

  /* A task list draws its own boxes, so it doesn't want bullets too. */
  .md :global(li.task) {
    list-style: none;
    margin-left: -1.2em;
  }

  .md :global(li.task input) {
    margin: 0 0.35em 0 0;
    vertical-align: -0.1em;
    accent-color: var(--accent);
  }

  .md :global(blockquote) {
    margin: 0 0 0.6em;
    padding: 0.05em 0 0.05em 0.85em;
    border-left: 2px solid var(--border);
    color: var(--fg-muted);
  }

  .md :global(hr) {
    border: none;
    border-top: 1px solid var(--border);
    margin: 0.9em 0;
  }

  .md :global(code) {
    font-family: var(--font-mono);
    font-size: 0.88em;
    background: var(--code-bg);
    padding: 0.1em 0.35em;
    border-radius: var(--radius-xs);
    overflow-wrap: anywhere;
  }

  .md :global(pre) {
    margin: 0 0 0.6em;
    padding: 0.55rem 0.7rem;
    background: var(--code-bg);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    font-size: 0.85em;
    line-height: var(--leading-normal);
    /* Code keeps the line breaks it was written with and scrolls instead;
       re-wrapping it would misrepresent what the agent wrote. */
    overflow-x: auto;
  }

  .md :global(pre code) {
    background: none;
    padding: 0;
    font-size: 1em;
    white-space: pre;
    overflow-wrap: normal;
  }

  .md :global(.codeblock) {
    position: relative;
  }

  .md :global(.codeblock.labelled pre) {
    padding-right: 3.4rem;
  }

  .md :global(.codeblock .lang) {
    position: absolute;
    top: 0.35rem;
    right: 0.55rem;
    font-family: var(--font-mono);
    font-size: 0.7em;
    color: var(--fg-muted);
    pointer-events: none;
  }

  .md :global(.table-wrap) {
    overflow-x: auto;
    margin: 0 0 0.6em;
  }

  .md :global(table) {
    border-collapse: collapse;
    font-size: 0.92em;
  }

  .md :global(th),
  .md :global(td) {
    border: 1px solid var(--border);
    padding: 0.25em 0.6em;
    text-align: left;
    vertical-align: top;
  }

  .md :global(th) {
    background: var(--panel-bg);
    font-weight: var(--weight-semibold);
  }

  .md :global(strong) {
    font-weight: var(--weight-semibold);
  }

  .md :global(del) {
    opacity: 0.7;
  }

  .md :global(.html-literal) {
    margin: 0 0 0.6em;
    font-family: var(--font-mono);
    font-size: 0.88em;
    color: var(--fg-muted);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
</style>
