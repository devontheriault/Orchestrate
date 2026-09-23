<script lang="ts">
  import CodeSpans from "./CodeSpans.svelte";
  import { highlightLines } from "./highlight.svelte";

  /**
   * Lines of a file beside their line numbers, as Read hands them back. The
   * numbers sit in a gutter of their own — out of the way of the code, and
   * out of a copied selection. Goes inside the caller's `<pre>`.
   */
  let { lines, lang }: { lines: { n: string; text: string }[]; lang: string } = $props();

  const spans = $derived(highlightLines(lines.map((l) => l.text), lang));
</script>

<span class="numbered"
  >{#each lines as l, i}<span class="n">{l.n}</span><code
      >{#if spans?.[i]}<CodeSpans spans={spans[i]} />{:else}{l.text}{/if}</code
    >{/each}</span
>

<style>
  /* A long line wraps under itself, not under its number. */
  .numbered {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    column-gap: 0.9em;
  }

  .n {
    text-align: right;
    color: var(--fg-muted);
    opacity: 0.6;
    user-select: none;
  }
</style>
